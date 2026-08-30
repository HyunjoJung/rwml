#!/usr/bin/env python3
"""Bounded content-free PDF point-geometry and semantic diagnostics."""

from __future__ import annotations

import unicodedata
from collections import Counter
from collections.abc import Sequence
from dataclasses import dataclass
from decimal import Decimal, InvalidOperation, ROUND_HALF_UP


DIAGNOSTIC_SCHEMA = "rwml.pdf-diagnostics.v2"
PPM = 1_000_000
MAX_POINT_MAGNITUDE = 1_000_000
MAX_POINT_MILLIPOINTS = MAX_POINT_MAGNITUDE * 1000
MAX_SEMANTIC_CODEPOINTS = 1_000_000
MAX_SEMANTIC_TOKENS = 250_000
MAX_TEXT_GEOMETRY_ITEMS = 250_000
MAX_TEXT_GEOMETRY_BUCKETS = 21
MAX_TEXT_GEOMETRY_PAGES = 4_096
MAX_TEXT_GEOMETRY_DELTA_MILLIPOINTS = 1_000_000_000
TEXT_GEOMETRY_EXACT_LIMIT_MILLIPOINTS = 2
TEXT_GEOMETRY_MIDDLE_LIMIT_MILLIPOINTS = 1_000
TEXT_GEOMETRY_MIDDLE_BUCKET_MILLIPOINTS = 500
TEXT_GEOMETRY_OUTER_LIMIT_MILLIPOINTS = 10_000
TEXT_GEOMETRY_OUTER_BUCKET_MILLIPOINTS = 2_000
TEXT_GEOMETRY_OVERFLOW_MILLIPOINTS = 12_000
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
TEXT_GEOMETRY_AXES = (
    "x_min",
    "x_max",
    "y_min",
    "y_max",
    "center_x",
    "center_y",
    "width",
    "height",
)
TEXT_GEOMETRY_COUNT_KEYS = frozenset(
    {
        "reference_items",
        "candidate_items",
        "reference_unique_items",
        "candidate_unique_items",
        "reference_ambiguous_items",
        "candidate_ambiguous_items",
        "reference_unmatched_unique_items",
        "candidate_unmatched_unique_items",
        "matched_items",
        "precision_ppm",
        "recall_ppm",
        "f1_ppm",
    }
)
TEXT_GEOMETRY_METRIC_KEYS = frozenset(
    set(TEXT_GEOMETRY_COUNT_KEYS)
    | {
        "delta_histograms_millipoints",
        "exact_delta_summaries_millipoints",
    }
)
TEXT_GEOMETRY_EXACT_SUMMARY_KEYS = frozenset(
    {
        "count",
        "sum",
        "min",
        "max",
        "negative_overflow_items",
        "positive_overflow_items",
    }
)
TEXT_GEOMETRY_PAGE_KEYS = frozenset({"word_boxes", "line_boxes"})
TEXT_GEOMETRY_REPORT_KEYS = frozenset({"pages", "summary"})
TEXT_GEOMETRY_REPORT_SUMMARY_KEYS = frozenset(
    {"pages", "word_boxes", "line_boxes"}
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
        "text_geometry_matching",
        "text_geometry_axes",
        "max_text_geometry_items_per_side_per_page",
        "max_text_geometry_histogram_buckets_per_axis",
        "text_geometry_histogram_millipoints",
    }
)


@dataclass(frozen=True)
class SemanticTextBox:
    tokens: tuple[str, ...]
    bbox_millipoints: tuple[int, int, int, int]


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
        "text_geometry_matching": "exact-token-tuple-unique-on-both-sides",
        "text_geometry_axes": list(TEXT_GEOMETRY_AXES),
        "max_text_geometry_items_per_side_per_page": MAX_TEXT_GEOMETRY_ITEMS,
        "max_text_geometry_histogram_buckets_per_axis": MAX_TEXT_GEOMETRY_BUCKETS,
        "text_geometry_histogram_millipoints": {
            "exact_absolute_limit": TEXT_GEOMETRY_EXACT_LIMIT_MILLIPOINTS,
            "middle_absolute_limit": TEXT_GEOMETRY_MIDDLE_LIMIT_MILLIPOINTS,
            "middle_bucket_width": TEXT_GEOMETRY_MIDDLE_BUCKET_MILLIPOINTS,
            "outer_absolute_limit": TEXT_GEOMETRY_OUTER_LIMIT_MILLIPOINTS,
            "outer_bucket_width": TEXT_GEOMETRY_OUTER_BUCKET_MILLIPOINTS,
            "overflow_bucket_absolute": TEXT_GEOMETRY_OVERFLOW_MILLIPOINTS,
        },
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


def canonical_text_box(
    tokens: Sequence[str], bbox_points: Sequence[object]
) -> SemanticTextBox:
    normalized = _validate_tokens(tokens, "text box")
    if not normalized:
        raise ValueError("text box tokens are empty")
    return SemanticTextBox(normalized, _box_millipoints(bbox_points, "text"))


def _signed_round_half_away(numerator: int, denominator: int) -> int:
    if denominator <= 0:
        raise ValueError("signed rounding denominator is invalid")
    magnitude = (abs(numerator) + denominator // 2) // denominator
    return -magnitude if numerator < 0 else magnitude


def _text_geometry_bucket(delta: int) -> int:
    magnitude = abs(delta)
    if magnitude <= TEXT_GEOMETRY_EXACT_LIMIT_MILLIPOINTS:
        return delta
    if magnitude <= TEXT_GEOMETRY_MIDDLE_LIMIT_MILLIPOINTS:
        width = TEXT_GEOMETRY_MIDDLE_BUCKET_MILLIPOINTS
        bucket = max(width, (magnitude + width // 2) // width * width)
        return -bucket if delta < 0 else bucket
    if magnitude <= TEXT_GEOMETRY_OUTER_LIMIT_MILLIPOINTS:
        width = TEXT_GEOMETRY_OUTER_BUCKET_MILLIPOINTS
        bucket = (magnitude + width // 2) // width * width
        return -bucket if delta < 0 else bucket
    return (
        -TEXT_GEOMETRY_OVERFLOW_MILLIPOINTS
        if delta < 0
        else TEXT_GEOMETRY_OVERFLOW_MILLIPOINTS
    )


def _text_geometry_ratio_evidence(
    reference_unique: int, candidate_unique: int, matched: int
) -> dict[str, int]:
    both_empty = reference_unique == 0 and candidate_unique == 0
    return {
        "precision_ppm": _ratio_ppm(
            matched,
            candidate_unique,
            empty=PPM if both_empty else 0,
        ),
        "recall_ppm": _ratio_ppm(
            matched,
            reference_unique,
            empty=PPM if both_empty else 0,
        ),
        "f1_ppm": _ratio_ppm(
            2 * matched,
            reference_unique + candidate_unique,
            empty=PPM if both_empty else 0,
        ),
    }


def _text_box_deltas(
    reference: SemanticTextBox, candidate: SemanticTextBox
) -> dict[str, int]:
    reference_x0, reference_y0, reference_x1, reference_y1 = (
        reference.bbox_millipoints
    )
    candidate_x0, candidate_y0, candidate_x1, candidate_y1 = (
        candidate.bbox_millipoints
    )
    return {
        "x_min": candidate_x0 - reference_x0,
        "x_max": candidate_x1 - reference_x1,
        "y_min": candidate_y0 - reference_y0,
        "y_max": candidate_y1 - reference_y1,
        "center_x": _signed_round_half_away(
            (candidate_x0 + candidate_x1) - (reference_x0 + reference_x1),
            2,
        ),
        "center_y": _signed_round_half_away(
            (candidate_y0 + candidate_y1) - (reference_y0 + reference_y1),
            2,
        ),
        "width": (candidate_x1 - candidate_x0) - (reference_x1 - reference_x0),
        "height": (candidate_y1 - candidate_y0) - (reference_y1 - reference_y0),
    }


def _empty_text_geometry_summaries() -> dict[str, dict[str, int | None]]:
    return {
        axis: {
            "count": 0,
            "sum": 0,
            "min": None,
            "max": None,
            "negative_overflow_items": 0,
            "positive_overflow_items": 0,
        }
        for axis in TEXT_GEOMETRY_AXES
    }


def unique_text_geometry_metrics(
    reference_boxes: Sequence[SemanticTextBox],
    candidate_boxes: Sequence[SemanticTextBox],
    *,
    max_items: int = MAX_TEXT_GEOMETRY_ITEMS,
) -> dict[str, object]:
    if (
        not isinstance(max_items, int)
        or isinstance(max_items, bool)
        or not 1 <= max_items <= MAX_TEXT_GEOMETRY_ITEMS
        or len(reference_boxes) > max_items
        or len(candidate_boxes) > max_items
    ):
        raise ValueError("text geometry item limit exceeded")
    if any(not isinstance(box, SemanticTextBox) for box in reference_boxes):
        raise ValueError("reference text geometry item is invalid")
    if any(not isinstance(box, SemanticTextBox) for box in candidate_boxes):
        raise ValueError("candidate text geometry item is invalid")

    reference_counts = Counter(box.tokens for box in reference_boxes)
    candidate_counts = Counter(box.tokens for box in candidate_boxes)
    reference_unique = {
        box.tokens: box
        for box in reference_boxes
        if reference_counts[box.tokens] == 1
    }
    candidate_unique = {
        box.tokens: box
        for box in candidate_boxes
        if candidate_counts[box.tokens] == 1
    }
    histograms = {axis: Counter() for axis in TEXT_GEOMETRY_AXES}
    summaries = _empty_text_geometry_summaries()
    matched = 0
    for tokens, candidate in candidate_unique.items():
        reference = reference_unique.get(tokens)
        if reference is None:
            continue
        deltas = _text_box_deltas(reference, candidate)
        for axis, delta in deltas.items():
            if abs(delta) > MAX_TEXT_GEOMETRY_DELTA_MILLIPOINTS:
                raise ValueError("text geometry delta limit exceeded")
            histograms[axis][_text_geometry_bucket(delta)] += 1
            summary = summaries[axis]
            summary["count"] += 1
            summary["sum"] += delta
            summary["min"] = (
                delta if summary["min"] is None else min(summary["min"], delta)
            )
            summary["max"] = (
                delta if summary["max"] is None else max(summary["max"], delta)
            )
            summary["negative_overflow_items"] += int(
                delta < -TEXT_GEOMETRY_OUTER_LIMIT_MILLIPOINTS
            )
            summary["positive_overflow_items"] += int(
                delta > TEXT_GEOMETRY_OUTER_LIMIT_MILLIPOINTS
            )
        matched += 1

    evidence: dict[str, object] = {
        "reference_items": len(reference_boxes),
        "candidate_items": len(candidate_boxes),
        "reference_unique_items": len(reference_unique),
        "candidate_unique_items": len(candidate_unique),
        "reference_ambiguous_items": len(reference_boxes) - len(reference_unique),
        "candidate_ambiguous_items": len(candidate_boxes) - len(candidate_unique),
        "reference_unmatched_unique_items": len(reference_unique) - matched,
        "candidate_unmatched_unique_items": len(candidate_unique) - matched,
        "matched_items": matched,
        **_text_geometry_ratio_evidence(
            len(reference_unique), len(candidate_unique), matched
        ),
        "delta_histograms_millipoints": {
            axis: [
                {"delta_millipoints": delta, "count": count}
                for delta, count in sorted(histograms[axis].items())
            ]
            for axis in TEXT_GEOMETRY_AXES
        },
        "exact_delta_summaries_millipoints": summaries,
    }
    validate_unique_text_geometry_metrics(evidence)
    return evidence


def validate_unique_text_geometry_metrics(value: object) -> None:
    if not isinstance(value, dict) or set(value) != TEXT_GEOMETRY_METRIC_KEYS:
        raise ValueError("text geometry metric keys are invalid")
    for key in TEXT_GEOMETRY_COUNT_KEYS:
        item = value[key]
        if not isinstance(item, int) or isinstance(item, bool) or item < 0:
            raise ValueError(f"text geometry metric is invalid: {key}")
        if key.endswith("_ppm") and item > PPM:
            raise ValueError(f"text geometry metric is out of range: {key}")
    if value["reference_items"] != (
        value["reference_unique_items"] + value["reference_ambiguous_items"]
    ):
        raise ValueError("reference text geometry counts are inconsistent")
    if value["candidate_items"] != (
        value["candidate_unique_items"] + value["candidate_ambiguous_items"]
    ):
        raise ValueError("candidate text geometry counts are inconsistent")
    matched = value["matched_items"]
    if matched > min(
        value["reference_unique_items"], value["candidate_unique_items"]
    ):
        raise ValueError("text geometry matched count is invalid")
    if value["reference_unmatched_unique_items"] != (
        value["reference_unique_items"] - matched
    ):
        raise ValueError("reference unmatched text geometry count is inconsistent")
    if value["candidate_unmatched_unique_items"] != (
        value["candidate_unique_items"] - matched
    ):
        raise ValueError("candidate unmatched text geometry count is inconsistent")
    expected_ratios = _text_geometry_ratio_evidence(
        value["reference_unique_items"], value["candidate_unique_items"], matched
    )
    for key, expected in expected_ratios.items():
        if value[key] != expected:
            raise ValueError(f"text geometry metric is inconsistent: {key}")

    histograms = value["delta_histograms_millipoints"]
    summaries = value["exact_delta_summaries_millipoints"]
    if not isinstance(histograms, dict) or set(histograms) != set(TEXT_GEOMETRY_AXES):
        raise ValueError("text geometry histograms are invalid")
    if not isinstance(summaries, dict) or set(summaries) != set(TEXT_GEOMETRY_AXES):
        raise ValueError("text geometry exact summaries are invalid")
    for axis in TEXT_GEOMETRY_AXES:
        rows = histograms[axis]
        if not isinstance(rows, list) or len(rows) > MAX_TEXT_GEOMETRY_BUCKETS:
            raise ValueError(f"text geometry histogram is invalid: {axis}")
        previous = None
        count = 0
        for row in rows:
            if not isinstance(row, dict) or set(row) != {"delta_millipoints", "count"}:
                raise ValueError(f"text geometry histogram row is invalid: {axis}")
            delta = row["delta_millipoints"]
            row_count = row["count"]
            if (
                not isinstance(delta, int)
                or isinstance(delta, bool)
                or _text_geometry_bucket(delta) != delta
                or not isinstance(row_count, int)
                or isinstance(row_count, bool)
                or row_count <= 0
                or (previous is not None and delta <= previous)
            ):
                raise ValueError(f"text geometry histogram row is invalid: {axis}")
            previous = delta
            count += row_count
        if count != matched:
            raise ValueError(f"text geometry histogram count is inconsistent: {axis}")

        summary = summaries[axis]
        if not isinstance(summary, dict) or set(summary) != TEXT_GEOMETRY_EXACT_SUMMARY_KEYS:
            raise ValueError(f"text geometry exact summary is invalid: {axis}")
        if summary["count"] != matched:
            raise ValueError(f"text geometry exact summary count is inconsistent: {axis}")
        for key in ("count", "sum", "negative_overflow_items", "positive_overflow_items"):
            if not isinstance(summary[key], int) or isinstance(summary[key], bool):
                raise ValueError(f"text geometry exact summary value is invalid: {axis}")
        if not 0 <= summary["negative_overflow_items"] <= matched:
            raise ValueError(f"text geometry negative overflow count is invalid: {axis}")
        if not 0 <= summary["positive_overflow_items"] <= matched:
            raise ValueError(f"text geometry positive overflow count is invalid: {axis}")
        if matched == 0:
            if summary["min"] is not None or summary["max"] is not None or summary["sum"] != 0:
                raise ValueError(f"empty text geometry summary is invalid: {axis}")
        elif (
            not isinstance(summary["min"], int)
            or isinstance(summary["min"], bool)
            or not isinstance(summary["max"], int)
            or isinstance(summary["max"], bool)
            or summary["min"] > summary["max"]
            or abs(summary["min"]) > MAX_TEXT_GEOMETRY_DELTA_MILLIPOINTS
            or abs(summary["max"]) > MAX_TEXT_GEOMETRY_DELTA_MILLIPOINTS
        ):
            raise ValueError(f"text geometry exact summary range is invalid: {axis}")


def _aggregate_unique_text_geometry_metrics(
    rows: Sequence[dict[str, object]],
) -> dict[str, object]:
    if not rows:
        raise ValueError("text geometry aggregation requires rows")
    for row in rows:
        validate_unique_text_geometry_metrics(row)
    reference_unique = sum(row["reference_unique_items"] for row in rows)
    candidate_unique = sum(row["candidate_unique_items"] for row in rows)
    matched = sum(row["matched_items"] for row in rows)
    histogram_counters = {axis: Counter() for axis in TEXT_GEOMETRY_AXES}
    summaries = _empty_text_geometry_summaries()
    for row in rows:
        for axis in TEXT_GEOMETRY_AXES:
            for bucket in row["delta_histograms_millipoints"][axis]:
                histogram_counters[axis][bucket["delta_millipoints"]] += bucket["count"]
            source = row["exact_delta_summaries_millipoints"][axis]
            target = summaries[axis]
            target["count"] += source["count"]
            target["sum"] += source["sum"]
            target["negative_overflow_items"] += source["negative_overflow_items"]
            target["positive_overflow_items"] += source["positive_overflow_items"]
            if source["min"] is not None:
                target["min"] = source["min"] if target["min"] is None else min(target["min"], source["min"])
                target["max"] = source["max"] if target["max"] is None else max(target["max"], source["max"])
    evidence: dict[str, object] = {
        "reference_items": sum(row["reference_items"] for row in rows),
        "candidate_items": sum(row["candidate_items"] for row in rows),
        "reference_unique_items": reference_unique,
        "candidate_unique_items": candidate_unique,
        "reference_ambiguous_items": sum(row["reference_ambiguous_items"] for row in rows),
        "candidate_ambiguous_items": sum(row["candidate_ambiguous_items"] for row in rows),
        "reference_unmatched_unique_items": reference_unique - matched,
        "candidate_unmatched_unique_items": candidate_unique - matched,
        "matched_items": matched,
        **_text_geometry_ratio_evidence(reference_unique, candidate_unique, matched),
        "delta_histograms_millipoints": {
            axis: [
                {"delta_millipoints": delta, "count": count}
                for delta, count in sorted(histogram_counters[axis].items())
            ]
            for axis in TEXT_GEOMETRY_AXES
        },
        "exact_delta_summaries_millipoints": summaries,
    }
    validate_unique_text_geometry_metrics(evidence)
    return evidence


def text_geometry_page(
    reference_word_boxes: Sequence[SemanticTextBox],
    candidate_word_boxes: Sequence[SemanticTextBox],
    reference_line_boxes: Sequence[SemanticTextBox],
    candidate_line_boxes: Sequence[SemanticTextBox],
) -> dict[str, object]:
    page = {
        "word_boxes": unique_text_geometry_metrics(
            reference_word_boxes, candidate_word_boxes
        ),
        "line_boxes": unique_text_geometry_metrics(
            reference_line_boxes, candidate_line_boxes
        ),
    }
    validate_text_geometry_page(page)
    return page


def validate_text_geometry_page(value: object) -> None:
    if not isinstance(value, dict) or set(value) != TEXT_GEOMETRY_PAGE_KEYS:
        raise ValueError("text geometry page keys are invalid")
    validate_unique_text_geometry_metrics(value["word_boxes"])
    validate_unique_text_geometry_metrics(value["line_boxes"])


def _text_geometry_summary(pages: Sequence[dict[str, object]]) -> dict[str, object]:
    return {
        "pages": len(pages),
        "word_boxes": _aggregate_unique_text_geometry_metrics(
            [page["word_boxes"] for page in pages]
        ),
        "line_boxes": _aggregate_unique_text_geometry_metrics(
            [page["line_boxes"] for page in pages]
        ),
    }


def text_geometry_report(
    pages: Sequence[dict[str, object]],
) -> dict[str, object]:
    if not pages or len(pages) > MAX_TEXT_GEOMETRY_PAGES:
        raise ValueError("text geometry report page limit exceeded")
    for page in pages:
        validate_text_geometry_page(page)
    report = {"pages": list(pages), "summary": _text_geometry_summary(pages)}
    validate_text_geometry_report(report)
    return report


def validate_text_geometry_summary(value: object) -> None:
    if not isinstance(value, dict) or set(value) != TEXT_GEOMETRY_REPORT_SUMMARY_KEYS:
        raise ValueError("text geometry report summary keys are invalid")
    if (
        not isinstance(value["pages"], int)
        or isinstance(value["pages"], bool)
        or not 1 <= value["pages"] <= MAX_TEXT_GEOMETRY_PAGES
    ):
        raise ValueError("text geometry report page count is invalid")
    validate_unique_text_geometry_metrics(value["word_boxes"])
    validate_unique_text_geometry_metrics(value["line_boxes"])


def validate_text_geometry_report(value: object) -> None:
    if not isinstance(value, dict) or set(value) != TEXT_GEOMETRY_REPORT_KEYS:
        raise ValueError("text geometry report keys are invalid")
    pages = value["pages"]
    if not isinstance(pages, list) or not pages or len(pages) > MAX_TEXT_GEOMETRY_PAGES:
        raise ValueError("text geometry report pages are invalid")
    for page in pages:
        validate_text_geometry_page(page)
    validate_text_geometry_summary(value["summary"])
    if value["summary"] != _text_geometry_summary(pages):
        raise ValueError("text geometry report summary is inconsistent")


def aggregate_text_geometry_reports(
    reports: Sequence[dict[str, object]],
) -> dict[str, object]:
    if not reports:
        raise ValueError("text geometry report aggregation requires reports")
    pages = []
    for report in reports:
        validate_text_geometry_report(report)
        pages.extend(report["pages"])
    if len(pages) > MAX_TEXT_GEOMETRY_PAGES:
        raise ValueError("text geometry aggregate page limit exceeded")
    return _text_geometry_summary(pages)
