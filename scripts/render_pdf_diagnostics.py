#!/usr/bin/env python3
"""Bounded content-free PDF point-geometry and semantic diagnostics."""

from __future__ import annotations

import unicodedata
from collections import Counter
from collections.abc import Sequence
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP


DIAGNOSTIC_SCHEMA = "rwml.pdf-diagnostics.v1"
PPM = 1_000_000
MAX_POINT_MAGNITUDE = 1_000_000
MAX_POINT_MILLIPOINTS = MAX_POINT_MAGNITUDE * 1000
MAX_SEMANTIC_CODEPOINTS = 1_000_000
MAX_SEMANTIC_TOKENS = 250_000
VALID_ROTATIONS = frozenset({0, 90, 180, 270})
SEMANTIC_IGNORED_CODEPOINTS = frozenset(
    {
        "\u00ad",
        "\u061c",
        "\u200e",
        "\u200f",
        "\u202a",
        "\u202b",
        "\u202c",
        "\u202d",
        "\u202e",
        "\u2066",
        "\u2067",
        "\u2068",
        "\u2069",
        "\ufeff",
    }
)

POINT_AXES = (
    "page_width",
    "page_height",
    "media_x0",
    "media_y0",
    "media_x1",
    "media_y1",
    "crop_x0",
    "crop_y0",
    "crop_x1",
    "crop_y1",
)
PAGE_GEOMETRY_KEYS = frozenset(
    {f"{axis}_millipoints" for axis in POINT_AXES} | {"rotation_degrees"}
)
PAGE_GEOMETRY_METRIC_KEYS = frozenset(
    {
        "reference",
        "candidate",
        "delta_millipoints",
        "rotation_delta_degrees",
    }
)
AXIS_SUMMARY_KEYS = frozenset({"sum", "min", "max", "nonzero_pages"})
GEOMETRY_SUMMARY_KEYS = frozenset(
    {
        "pages",
        "point_mismatched_pages",
        "rotation_mismatched_pages",
        "max_abs_delta_millipoints",
        "sum_abs_delta_millipoints",
        "delta_summaries_millipoints",
    }
)
GEOMETRY_REPORT_KEYS = frozenset({"pages", "summary"})

SEMANTIC_PREFIXES = ("token", "codepoint", "bigram")
SEMANTIC_RATIO_SUFFIXES = (
    "reference_items",
    "candidate_items",
    "matched_items",
    "precision_ppm",
    "recall_ppm",
    "f1_ppm",
)
SEMANTIC_METRIC_KEYS = frozenset(
    {"semantic_exact", "semantic_comparable", "semantic_one_sided_empty"}
    | {
        f"semantic_{prefix}_{suffix}"
        for prefix in SEMANTIC_PREFIXES
        for suffix in SEMANTIC_RATIO_SUFFIXES
    }
)
SEMANTIC_REPORT_KEYS = frozenset(
    {
        "pages",
        "semantic_exact",
        "semantic_exact_pages",
        "semantic_page_mismatches",
        "semantic_comparable",
        "semantic_comparable_pages",
        "semantic_one_sided_empty_pages",
    }
    | {
        f"semantic_{prefix}_{suffix}"
        for prefix in SEMANTIC_PREFIXES
        for suffix in SEMANTIC_RATIO_SUFFIXES
    }
)
DIAGNOSTIC_CONTRACT_KEYS = frozenset(
    {
        "schema",
        "content_retained",
        "point_unit",
        "point_rounding",
        "delta_direction",
        "max_point_magnitude",
        "semantic_normalization",
        "semantic_ignored_codepoints",
        "max_semantic_codepoints",
        "max_semantic_tokens",
    }
)


def diagnostic_contract() -> dict[str, object]:
    return {
        "schema": DIAGNOSTIC_SCHEMA,
        "content_retained": False,
        "point_unit": "millipoints",
        "point_rounding": "nearest-half-away-from-zero",
        "delta_direction": "candidate-minus-reference",
        "max_point_magnitude": MAX_POINT_MAGNITUDE,
        "semantic_normalization": "nfc-drop-layout-controls-split-whitespace-v1",
        "semantic_ignored_codepoints": [
            f"U+{ord(character):04X}"
            for character in sorted(SEMANTIC_IGNORED_CODEPOINTS)
        ],
        "max_semantic_codepoints": MAX_SEMANTIC_CODEPOINTS,
        "max_semantic_tokens": MAX_SEMANTIC_TOKENS,
    }


def validate_diagnostic_contract(value: object) -> None:
    if not isinstance(value, dict) or set(value) != DIAGNOSTIC_CONTRACT_KEYS:
        raise ValueError("PDF diagnostic contract keys are invalid")
    if value != diagnostic_contract():
        raise ValueError("PDF diagnostic contract is invalid")


def _millipoints(value: object, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, (int, float, str, Decimal)):
        raise ValueError(f"point value is invalid: {name}")
    try:
        point = Decimal(str(value))
    except (InvalidOperation, ValueError) as error:
        raise ValueError(f"point value is invalid: {name}") from error
    if not point.is_finite() or abs(point) > MAX_POINT_MAGNITUDE:
        raise ValueError(f"point value is out of range: {name}")
    return int((point * 1000).to_integral_value(rounding=ROUND_HALF_UP))


def _bounded_sequence(value: object, length: int, name: str) -> tuple[object, ...]:
    if isinstance(value, (str, bytes)):
        raise ValueError(f"{name} is invalid")
    try:
        result = tuple(value)
    except TypeError as error:
        raise ValueError(f"{name} is invalid") from error
    if len(result) != length:
        raise ValueError(f"{name} is invalid")
    return result


def _box_millipoints(value: Sequence[object], name: str) -> tuple[int, int, int, int]:
    items = _bounded_sequence(value, 4, f"{name} box")
    box = tuple(_millipoints(item, f"{name} box") for item in items)
    if box[2] <= box[0] or box[3] <= box[1]:
        raise ValueError(f"{name} box is degenerate")
    return box


def canonical_page_geometry(
    *,
    page_size: Sequence[object],
    media_box: Sequence[object],
    crop_box: Sequence[object],
    rotation_degrees: int,
) -> dict[str, int]:
    size = _bounded_sequence(page_size, 2, "page size")
    width = _millipoints(size[0], "page width")
    height = _millipoints(size[1], "page height")
    if width <= 0 or height <= 0:
        raise ValueError("page size is invalid")
    media = _box_millipoints(media_box, "media")
    crop = _box_millipoints(crop_box, "crop")
    if (
        not isinstance(rotation_degrees, int)
        or isinstance(rotation_degrees, bool)
        or rotation_degrees not in VALID_ROTATIONS
    ):
        raise ValueError("page rotation is invalid")
    result = {
        "page_width_millipoints": width,
        "page_height_millipoints": height,
        "media_x0_millipoints": media[0],
        "media_y0_millipoints": media[1],
        "media_x1_millipoints": media[2],
        "media_y1_millipoints": media[3],
        "crop_x0_millipoints": crop[0],
        "crop_y0_millipoints": crop[1],
        "crop_x1_millipoints": crop[2],
        "crop_y1_millipoints": crop[3],
        "rotation_degrees": rotation_degrees,
    }
    validate_page_geometry(result)
    return result


def validate_page_geometry(value: object) -> None:
    if not isinstance(value, dict) or set(value) != PAGE_GEOMETRY_KEYS:
        raise ValueError("page geometry keys are invalid")
    for key, item in value.items():
        if not isinstance(item, int) or isinstance(item, bool):
            raise ValueError(f"page geometry value is invalid: {key}")
        if key != "rotation_degrees" and abs(item) > MAX_POINT_MILLIPOINTS:
            raise ValueError(f"page geometry value is out of range: {key}")
    if value["rotation_degrees"] not in VALID_ROTATIONS:
        raise ValueError("page rotation is invalid")
    if value["page_width_millipoints"] <= 0 or value["page_height_millipoints"] <= 0:
        raise ValueError("page size is invalid")
    for prefix in ("media", "crop"):
        if (
            value[f"{prefix}_x1_millipoints"]
            <= value[f"{prefix}_x0_millipoints"]
            or value[f"{prefix}_y1_millipoints"]
            <= value[f"{prefix}_y0_millipoints"]
        ):
            raise ValueError(f"{prefix} box is degenerate")


def page_geometry_metrics(
    reference: dict[str, int], candidate: dict[str, int]
) -> dict[str, object]:
    validate_page_geometry(reference)
    validate_page_geometry(candidate)
    evidence = {
        "reference": dict(reference),
        "candidate": dict(candidate),
        "delta_millipoints": {
            axis: candidate[f"{axis}_millipoints"]
            - reference[f"{axis}_millipoints"]
            for axis in POINT_AXES
        },
        "rotation_delta_degrees": (
            candidate["rotation_degrees"] - reference["rotation_degrees"]
        ),
    }
    validate_page_geometry_metrics(evidence)
    return evidence


def validate_page_geometry_metrics(value: object) -> None:
    if not isinstance(value, dict) or set(value) != PAGE_GEOMETRY_METRIC_KEYS:
        raise ValueError("page geometry metric keys are invalid")
    validate_page_geometry(value["reference"])
    validate_page_geometry(value["candidate"])
    deltas = value["delta_millipoints"]
    if not isinstance(deltas, dict) or set(deltas) != set(POINT_AXES):
        raise ValueError("page geometry delta keys are invalid")
    for axis in POINT_AXES:
        expected = (
            value["candidate"][f"{axis}_millipoints"]
            - value["reference"][f"{axis}_millipoints"]
        )
        if deltas[axis] != expected:
            raise ValueError(f"page geometry delta is inconsistent: {axis}")
    expected_rotation = (
        value["candidate"]["rotation_degrees"]
        - value["reference"]["rotation_degrees"]
    )
    if value["rotation_delta_degrees"] != expected_rotation:
        raise ValueError("page geometry rotation delta is inconsistent")


def _geometry_summary(pages: Sequence[dict[str, object]]) -> dict[str, object]:
    delta_summaries = {}
    max_abs = 0
    sum_abs = 0
    for axis in POINT_AXES:
        values = [int(page["delta_millipoints"][axis]) for page in pages]
        delta_summaries[axis] = {
            "sum": sum(values),
            "min": min(values),
            "max": max(values),
            "nonzero_pages": sum(value != 0 for value in values),
        }
        max_abs = max(max_abs, *(abs(value) for value in values))
        sum_abs += sum(abs(value) for value in values)
    return {
        "pages": len(pages),
        "point_mismatched_pages": sum(
            any(page["delta_millipoints"][axis] != 0 for axis in POINT_AXES)
            for page in pages
        ),
        "rotation_mismatched_pages": sum(
            page["rotation_delta_degrees"] != 0 for page in pages
        ),
        "max_abs_delta_millipoints": max_abs,
        "sum_abs_delta_millipoints": sum_abs,
        "delta_summaries_millipoints": delta_summaries,
    }


def geometry_report(pages: Sequence[dict[str, object]]) -> dict[str, object]:
    if not pages:
        raise ValueError("geometry report requires at least one page")
    for page in pages:
        validate_page_geometry_metrics(page)
    report = {"pages": list(pages), "summary": _geometry_summary(pages)}
    validate_geometry_report(report)
    return report


def validate_geometry_summary(value: object) -> None:
    if not isinstance(value, dict) or set(value) != GEOMETRY_SUMMARY_KEYS:
        raise ValueError("geometry summary keys are invalid")
    for key in (
        "pages",
        "point_mismatched_pages",
        "rotation_mismatched_pages",
        "max_abs_delta_millipoints",
        "sum_abs_delta_millipoints",
    ):
        if not isinstance(value[key], int) or isinstance(value[key], bool) or value[key] < 0:
            raise ValueError(f"geometry summary value is invalid: {key}")
    summaries = value["delta_summaries_millipoints"]
    if not isinstance(summaries, dict) or set(summaries) != set(POINT_AXES):
        raise ValueError("geometry axis summaries are invalid")
    for axis, summary in summaries.items():
        if not isinstance(summary, dict) or set(summary) != AXIS_SUMMARY_KEYS:
            raise ValueError(f"geometry axis summary is invalid: {axis}")
        if any(
            not isinstance(item, int) or isinstance(item, bool)
            for item in summary.values()
        ):
            raise ValueError(f"geometry axis summary value is invalid: {axis}")
        if not 0 <= summary["nonzero_pages"] <= value["pages"]:
            raise ValueError(f"geometry axis summary count is invalid: {axis}")


def validate_geometry_report(value: object) -> None:
    if not isinstance(value, dict) or set(value) != GEOMETRY_REPORT_KEYS:
        raise ValueError("geometry report keys are invalid")
    pages = value["pages"]
    if not isinstance(pages, list) or not pages:
        raise ValueError("geometry report pages are invalid")
    for page in pages:
        validate_page_geometry_metrics(page)
    validate_geometry_summary(value["summary"])
    if value["summary"] != _geometry_summary(pages):
        raise ValueError("geometry report summary is inconsistent")


def aggregate_geometry_reports(
    reports: Sequence[dict[str, object]],
) -> dict[str, object]:
    if not reports:
        raise ValueError("geometry aggregation requires at least one report")
    pages = []
    for report in reports:
        validate_geometry_report(report)
        pages.extend(report["pages"])
    return _geometry_summary(pages)


def normalize_semantic_tokens(
    text: str, *, max_codepoints: int, max_tokens: int
) -> tuple[str, ...]:
    if not isinstance(text, str):
        raise ValueError("semantic text is invalid")
    for name, value, maximum in (
        ("codepoint", max_codepoints, MAX_SEMANTIC_CODEPOINTS),
        ("token", max_tokens, MAX_SEMANTIC_TOKENS),
    ):
        if (
            not isinstance(value, int)
            or isinstance(value, bool)
            or value < 0
            or value > maximum
        ):
            raise ValueError(f"semantic {name} limit is invalid")
    if len(text) > max_codepoints * 4:
        raise ValueError("semantic raw codepoint limit exceeded")
    normalized = unicodedata.normalize("NFC", text)
    normalized = "".join(
        character
        for character in normalized
        if character not in SEMANTIC_IGNORED_CODEPOINTS
    )
    tokens = tuple(normalized.split())
    if len(tokens) > max_tokens:
        raise ValueError("semantic token limit exceeded")
    if sum(len(token) for token in tokens) > max_codepoints:
        raise ValueError("semantic codepoint limit exceeded")
    return tokens


def _ratio_ppm(numerator: int, denominator: int, *, empty: int = 0) -> int:
    if denominator == 0:
        return empty
    return (numerator * PPM + denominator // 2) // denominator


def _semantic_ratio_evidence(
    prefix: str,
    reference_items: int,
    candidate_items: int,
    matched_items: int,
) -> dict[str, int]:
    both_empty = reference_items == 0 and candidate_items == 0
    return {
        f"semantic_{prefix}_reference_items": reference_items,
        f"semantic_{prefix}_candidate_items": candidate_items,
        f"semantic_{prefix}_matched_items": matched_items,
        f"semantic_{prefix}_precision_ppm": _ratio_ppm(
            matched_items,
            candidate_items,
            empty=PPM if both_empty else 0,
        ),
        f"semantic_{prefix}_recall_ppm": _ratio_ppm(
            matched_items,
            reference_items,
            empty=PPM if both_empty else 0,
        ),
        f"semantic_{prefix}_f1_ppm": _ratio_ppm(
            2 * matched_items,
            reference_items + candidate_items,
            empty=PPM if both_empty else 0,
        ),
    }


def _validate_tokens(tokens: Sequence[str], name: str) -> tuple[str, ...]:
    if isinstance(tokens, (str, bytes)):
        raise ValueError(f"semantic {name} tokens are invalid")
    result = tuple(tokens)
    if len(result) > MAX_SEMANTIC_TOKENS:
        raise ValueError(f"semantic {name} token limit exceeded")
    if any(not isinstance(token, str) or not token for token in result):
        raise ValueError(f"semantic {name} tokens are invalid")
    if sum(len(token) for token in result) > MAX_SEMANTIC_CODEPOINTS:
        raise ValueError(f"semantic {name} codepoint limit exceeded")
    return result


def semantic_metrics(
    reference_tokens: Sequence[str], candidate_tokens: Sequence[str]
) -> dict[str, int]:
    reference = _validate_tokens(reference_tokens, "reference")
    candidate = _validate_tokens(candidate_tokens, "candidate")
    reference_token_counter = Counter(reference)
    candidate_token_counter = Counter(candidate)
    matched_tokens = sum(
        (reference_token_counter & candidate_token_counter).values()
    )
    reference_codepoints = Counter("".join(reference))
    candidate_codepoints = Counter("".join(candidate))
    matched_codepoints = sum(
        (reference_codepoints & candidate_codepoints).values()
    )
    reference_bigrams = Counter(zip(reference, reference[1:]))
    candidate_bigrams = Counter(zip(candidate, candidate[1:]))
    matched_bigrams = sum((reference_bigrams & candidate_bigrams).values())
    one_sided_empty = bool(reference) != bool(candidate)
    evidence = {
        "semantic_exact": int(reference == candidate),
        "semantic_comparable": int(not one_sided_empty),
        "semantic_one_sided_empty": int(one_sided_empty),
    }
    evidence.update(
        _semantic_ratio_evidence(
            "token", len(reference), len(candidate), matched_tokens
        )
    )
    evidence.update(
        _semantic_ratio_evidence(
            "codepoint",
            sum(reference_codepoints.values()),
            sum(candidate_codepoints.values()),
            matched_codepoints,
        )
    )
    evidence.update(
        _semantic_ratio_evidence(
            "bigram",
            sum(reference_bigrams.values()),
            sum(candidate_bigrams.values()),
            matched_bigrams,
        )
    )
    validate_semantic_metrics(evidence)
    return evidence


def _validate_ratio_fields(value: dict[str, int], prefix: str) -> None:
    reference_items = value[f"semantic_{prefix}_reference_items"]
    candidate_items = value[f"semantic_{prefix}_candidate_items"]
    matched_items = value[f"semantic_{prefix}_matched_items"]
    if matched_items > min(reference_items, candidate_items):
        raise ValueError(f"semantic_{prefix}_matched_items is invalid")
    expected = _semantic_ratio_evidence(
        prefix, reference_items, candidate_items, matched_items
    )
    for key, item in expected.items():
        if value[key] != item:
            raise ValueError(f"{key} is inconsistent")


def validate_semantic_metrics(value: object) -> None:
    if not isinstance(value, dict) or set(value) != SEMANTIC_METRIC_KEYS:
        raise ValueError("semantic metric keys are invalid")
    for key, item in value.items():
        if not isinstance(item, int) or isinstance(item, bool) or item < 0:
            raise ValueError(f"semantic metric is invalid: {key}")
        if key.endswith("_ppm") and item > PPM:
            raise ValueError(f"semantic metric is out of range: {key}")
    for key in (
        "semantic_exact",
        "semantic_comparable",
        "semantic_one_sided_empty",
    ):
        if value[key] not in {0, 1}:
            raise ValueError(f"semantic metric flag is invalid: {key}")
    one_sided = (
        (value["semantic_token_reference_items"] == 0)
        != (value["semantic_token_candidate_items"] == 0)
    )
    if value["semantic_one_sided_empty"] != int(one_sided):
        raise ValueError("semantic one-sided-empty flag is inconsistent")
    if value["semantic_comparable"] != int(not one_sided):
        raise ValueError("semantic comparable flag is inconsistent")
    for prefix in SEMANTIC_PREFIXES:
        _validate_ratio_fields(value, prefix)


def _semantic_report(rows: Sequence[dict[str, int]]) -> dict[str, int]:
    evidence = {
        "pages": len(rows),
        "semantic_exact": int(all(row["semantic_exact"] for row in rows)),
        "semantic_exact_pages": sum(row["semantic_exact"] for row in rows),
        "semantic_page_mismatches": sum(
            not row["semantic_exact"] for row in rows
        ),
        "semantic_comparable": int(
            all(row["semantic_comparable"] for row in rows)
        ),
        "semantic_comparable_pages": sum(
            row["semantic_comparable"] for row in rows
        ),
        "semantic_one_sided_empty_pages": sum(
            row["semantic_one_sided_empty"] for row in rows
        ),
    }
    for prefix in SEMANTIC_PREFIXES:
        evidence.update(
            _semantic_ratio_evidence(
                prefix,
                sum(row[f"semantic_{prefix}_reference_items"] for row in rows),
                sum(row[f"semantic_{prefix}_candidate_items"] for row in rows),
                sum(row[f"semantic_{prefix}_matched_items"] for row in rows),
            )
        )
    return evidence


def semantic_report(rows: Sequence[dict[str, int]]) -> dict[str, int]:
    if not rows:
        raise ValueError("semantic report requires at least one page")
    for row in rows:
        validate_semantic_metrics(row)
    report = _semantic_report(rows)
    validate_semantic_report(report)
    return report


def validate_semantic_report(value: object) -> None:
    if not isinstance(value, dict) or set(value) != SEMANTIC_REPORT_KEYS:
        raise ValueError("semantic report keys are invalid")
    for key, item in value.items():
        if not isinstance(item, int) or isinstance(item, bool) or item < 0:
            raise ValueError(f"semantic report value is invalid: {key}")
        if key.endswith("_ppm") and item > PPM:
            raise ValueError(f"semantic report value is out of range: {key}")
    pages = value["pages"]
    if pages == 0:
        raise ValueError("semantic report requires pages")
    if value["semantic_exact_pages"] + value["semantic_page_mismatches"] != pages:
        raise ValueError("semantic page counts are inconsistent")
    if not 0 <= value["semantic_comparable_pages"] <= pages:
        raise ValueError("semantic comparable page count is invalid")
    if not 0 <= value["semantic_one_sided_empty_pages"] <= pages:
        raise ValueError("semantic empty page count is invalid")
    if value["semantic_exact"] != int(value["semantic_exact_pages"] == pages):
        raise ValueError("semantic exact report flag is inconsistent")
    if value["semantic_comparable"] != int(
        value["semantic_comparable_pages"] == pages
    ):
        raise ValueError("semantic comparable report flag is inconsistent")
    for prefix in SEMANTIC_PREFIXES:
        _validate_ratio_fields(value, prefix)


def aggregate_semantic_reports(
    reports: Sequence[dict[str, int]],
) -> dict[str, int]:
    if not reports:
        raise ValueError("semantic aggregation requires at least one report")
    for report in reports:
        validate_semantic_report(report)
    aggregate = {
        "pages": sum(report["pages"] for report in reports),
        "semantic_exact_pages": sum(
            report["semantic_exact_pages"] for report in reports
        ),
        "semantic_page_mismatches": sum(
            report["semantic_page_mismatches"] for report in reports
        ),
        "semantic_comparable_pages": sum(
            report["semantic_comparable_pages"] for report in reports
        ),
        "semantic_one_sided_empty_pages": sum(
            report["semantic_one_sided_empty_pages"] for report in reports
        ),
    }
    aggregate["semantic_exact"] = int(
        aggregate["semantic_exact_pages"] == aggregate["pages"]
    )
    aggregate["semantic_comparable"] = int(
        aggregate["semantic_comparable_pages"] == aggregate["pages"]
    )
    for prefix in SEMANTIC_PREFIXES:
        aggregate.update(
            _semantic_ratio_evidence(
                prefix,
                sum(
                    report[f"semantic_{prefix}_reference_items"]
                    for report in reports
                ),
                sum(
                    report[f"semantic_{prefix}_candidate_items"]
                    for report in reports
                ),
                sum(
                    report[f"semantic_{prefix}_matched_items"]
                    for report in reports
                ),
            )
        )
    validate_semantic_report(aggregate)
    return aggregate
