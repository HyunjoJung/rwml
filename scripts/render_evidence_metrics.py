#!/usr/bin/env python3
"""Deterministic bounded integer metrics for render-oracle raster evidence."""

from __future__ import annotations

import math
from collections.abc import Sequence
from typing import Any


METRIC_SCHEMA = "rwml.render-integer-visual.v1"
FOREGROUND_CHANNEL_THRESHOLD = 248
EDGE_LUMA_DELTA = 32
TEXT_INK_MAX_LUMA = 192
METRIC_WORK_UNITS_PER_PIXEL = 128
PPM = 1_000_000

MASK_PREFIXES = ("foreground", "edge", "text_ink")
METRIC_KEYS = frozenset(
    {
        "pages",
        "pixels",
        "changed_pixels",
        "mismatch_ppm",
        "absolute_error_sum",
        "squared_error_sum",
        "max_channel_delta",
        "mean_absolute_error_ppm",
        "root_mean_square_error_ppm",
        "similarity_ppm",
        "metric_work_units",
        "foreground_matched_color_samples",
        "foreground_matched_color_absolute_error_sum",
        "foreground_matched_color_mean_absolute_error_ppm",
        "foreground_matched_color_similarity_ppm",
        "blurred_luma_absolute_error_sum",
        "blurred_luma_mean_absolute_error_ppm",
        "blurred_luma_similarity_ppm",
    }
    | {
        f"{prefix}_{suffix}"
        for prefix in MASK_PREFIXES
        for suffix in (
            "candidate_pixels",
            "reference_pixels",
            "candidate_matched_1px",
            "reference_matched_1px",
            "precision_ppm",
            "recall_ppm",
            "f1_ppm",
        )
    }
)


def numpy_module() -> Any | None:
    """Return NumPy when available without making it a harness dependency."""
    try:
        import numpy
    except ImportError:
        return None
    return numpy


def metric_implementation() -> str:
    if numpy_module() is None:
        return "python-integer-reference-v1"
    return "numpy-integer-exact-v1"


def metric_contract() -> dict[str, object]:
    return {
        "schema": METRIC_SCHEMA,
        "implementation": metric_implementation(),
        "foreground_channel_threshold": FOREGROUND_CHANNEL_THRESHOLD,
        "edge_luma_delta": EDGE_LUMA_DELTA,
        "text_ink_max_luma": TEXT_INK_MAX_LUMA,
        "match_radius_pixels": 1,
        "blur_kernel_pixels": 3,
        "work_units_per_pixel": METRIC_WORK_UNITS_PER_PIXEL,
    }


def _ratio_ppm(numerator: int, denominator: int, *, empty: int = 0) -> int:
    if denominator == 0:
        return empty
    return (numerator * PPM + denominator // 2) // denominator


def _validate_buffers(
    reference_rgb: bytes,
    candidate_rgb: bytes,
    width: int,
    height: int,
    max_metric_work_units: int | None,
) -> tuple[int, int]:
    if (
        not isinstance(width, int)
        or isinstance(width, bool)
        or not isinstance(height, int)
        or isinstance(height, bool)
        or width <= 0
        or height <= 0
    ):
        raise ValueError("metric dimensions are invalid")
    pixels = width * height
    if len(reference_rgb) != pixels * 3 or len(candidate_rgb) != len(reference_rgb):
        raise ValueError("metric buffer mismatch")
    work_units = pixels * METRIC_WORK_UNITS_PER_PIXEL
    if max_metric_work_units is not None:
        if (
            not isinstance(max_metric_work_units, int)
            or isinstance(max_metric_work_units, bool)
            or max_metric_work_units < 0
        ):
            raise ValueError("metric work limit is invalid")
        if work_units > max_metric_work_units:
            raise ValueError("metric work limit exceeded")
    return pixels, work_units


def _base_metrics(reference_rgb: bytes, candidate_rgb: bytes) -> dict[str, int]:
    pixels = len(reference_rgb) // 3
    changed_pixels = 0
    absolute_error_sum = 0
    squared_error_sum = 0
    max_channel_delta = 0
    for offset in range(0, len(reference_rgb), 3):
        changed = False
        for channel in range(3):
            delta = abs(reference_rgb[offset + channel] - candidate_rgb[offset + channel])
            changed = changed or delta != 0
            absolute_error_sum += delta
            squared_error_sum += delta * delta
            max_channel_delta = max(max_channel_delta, delta)
        changed_pixels += int(changed)
    channel_denominator = pixels * 3 * 255
    squared_denominator = pixels * 3 * 255 * 255
    mean_absolute_error_ppm = _ratio_ppm(
        absolute_error_sum, channel_denominator
    )
    return {
        "pixels": pixels,
        "changed_pixels": changed_pixels,
        "mismatch_ppm": _ratio_ppm(changed_pixels, pixels),
        "absolute_error_sum": absolute_error_sum,
        "squared_error_sum": squared_error_sum,
        "max_channel_delta": max_channel_delta,
        "mean_absolute_error_ppm": mean_absolute_error_ppm,
        "root_mean_square_error_ppm": math.isqrt(
            (squared_error_sum * PPM * PPM) // squared_denominator
        ),
        "similarity_ppm": max(0, PPM - mean_absolute_error_ppm),
    }


def _dilate_mask_1px(mask: bytes | bytearray, width: int, height: int) -> bytearray:
    horizontal = bytearray(width * height)
    for y in range(height):
        row = y * width
        for x in range(width):
            offset = row + x
            horizontal[offset] = int(
                bool(mask[offset])
                or (x > 0 and bool(mask[offset - 1]))
                or (x + 1 < width and bool(mask[offset + 1]))
            )
    expanded = bytearray(width * height)
    for y in range(height):
        row = y * width
        for x in range(width):
            offset = row + x
            expanded[offset] = int(
                bool(horizontal[offset])
                or (y > 0 and bool(horizontal[offset - width]))
                or (y + 1 < height and bool(horizontal[offset + width]))
            )
    return expanded


def _prf_evidence(
    prefix: str,
    candidate_pixels: int,
    reference_pixels: int,
    candidate_matched: int,
    reference_matched: int,
) -> dict[str, int]:
    both_empty = candidate_pixels == 0 and reference_pixels == 0
    precision = _ratio_ppm(
        candidate_matched,
        candidate_pixels,
        empty=PPM if both_empty else 0,
    )
    recall = _ratio_ppm(
        reference_matched,
        reference_pixels,
        empty=PPM if both_empty else 0,
    )
    f1_denominator = (
        candidate_matched * reference_pixels
        + reference_matched * candidate_pixels
    )
    if both_empty:
        f1 = PPM
    elif f1_denominator == 0:
        f1 = 0
    else:
        f1 = _ratio_ppm(
            2 * candidate_matched * reference_matched,
            f1_denominator,
        )
    return {
        f"{prefix}_candidate_pixels": candidate_pixels,
        f"{prefix}_reference_pixels": reference_pixels,
        f"{prefix}_candidate_matched_1px": candidate_matched,
        f"{prefix}_reference_matched_1px": reference_matched,
        f"{prefix}_precision_ppm": precision,
        f"{prefix}_recall_ppm": recall,
        f"{prefix}_f1_ppm": f1,
    }


def _mask_evidence(
    prefix: str,
    reference_mask: bytes | bytearray,
    candidate_mask: bytes | bytearray,
    width: int,
    height: int,
) -> dict[str, int]:
    reference_pixels = sum(bool(value) for value in reference_mask)
    candidate_pixels = sum(bool(value) for value in candidate_mask)
    dilated_reference = _dilate_mask_1px(reference_mask, width, height)
    dilated_candidate = _dilate_mask_1px(candidate_mask, width, height)
    candidate_matched = sum(
        bool(value) and bool(match)
        for value, match in zip(candidate_mask, dilated_reference)
    )
    reference_matched = sum(
        bool(value) and bool(match)
        for value, match in zip(reference_mask, dilated_candidate)
    )
    return _prf_evidence(
        prefix,
        candidate_pixels,
        reference_pixels,
        candidate_matched,
        reference_matched,
    )


def _luma_and_foreground(rgb: bytes) -> tuple[bytearray, bytearray]:
    luma = bytearray(len(rgb) // 3)
    foreground = bytearray(len(rgb) // 3)
    for pixel, offset in enumerate(range(0, len(rgb), 3)):
        red = rgb[offset]
        green = rgb[offset + 1]
        blue = rgb[offset + 2]
        luma[pixel] = (77 * red + 150 * green + 29 * blue + 128) >> 8
        foreground[pixel] = int(
            red < FOREGROUND_CHANNEL_THRESHOLD
            or green < FOREGROUND_CHANNEL_THRESHOLD
            or blue < FOREGROUND_CHANNEL_THRESHOLD
        )
    return luma, foreground


def _edge_mask(luma: bytes | bytearray, width: int, height: int) -> bytearray:
    mask = bytearray(width * height)
    for y in range(height):
        row = y * width
        for x in range(width):
            offset = row + x
            value = luma[offset]
            mask[offset] = int(
                (x > 0 and abs(value - luma[offset - 1]) >= EDGE_LUMA_DELTA)
                or (
                    x + 1 < width
                    and abs(value - luma[offset + 1]) >= EDGE_LUMA_DELTA
                )
                or (
                    y > 0
                    and abs(value - luma[offset - width]) >= EDGE_LUMA_DELTA
                )
                or (
                    y + 1 < height
                    and abs(value - luma[offset + width]) >= EDGE_LUMA_DELTA
                )
            )
    return mask


def _text_ink_mask(luma: bytes | bytearray, width: int, height: int) -> bytearray:
    mask = bytearray(width * height)
    for y in range(height):
        row = y * width
        for x in range(width):
            offset = row + x
            value = luma[offset]
            if value > TEXT_INK_MAX_LUMA:
                continue
            mask[offset] = int(
                (x > 0 and luma[offset - 1] - value >= EDGE_LUMA_DELTA)
                or (
                    x + 1 < width
                    and luma[offset + 1] - value >= EDGE_LUMA_DELTA
                )
                or (
                    y > 0
                    and luma[offset - width] - value >= EDGE_LUMA_DELTA
                )
                or (
                    y + 1 < height
                    and luma[offset + width] - value >= EDGE_LUMA_DELTA
                )
            )
    return mask


def _box_blur_luma_3px(
    luma: bytes | bytearray, width: int, height: int
) -> bytearray:
    horizontal = bytearray(width * height)
    for y in range(height):
        row = y * width
        window = luma[row]
        if width > 1:
            window += luma[row + 1]
        for x in range(width):
            count = 1 + int(x > 0) + int(x + 1 < width)
            horizontal[row + x] = (window + count // 2) // count
            if x > 0:
                window -= luma[row + x - 1]
            if x + 2 < width:
                window += luma[row + x + 2]

    blurred = bytearray(width * height)
    for x in range(width):
        window = horizontal[x]
        if height > 1:
            window += horizontal[width + x]
        for y in range(height):
            count = 1 + int(y > 0) + int(y + 1 < height)
            blurred[y * width + x] = (window + count // 2) // count
            if y > 0:
                window -= horizontal[(y - 1) * width + x]
            if y + 2 < height:
                window += horizontal[(y + 2) * width + x]
    return blurred


def _matched_foreground_color_error(
    reference_rgb: bytes,
    candidate_rgb: bytes,
    reference_mask: bytes | bytearray,
    candidate_mask: bytes | bytearray,
    width: int,
    height: int,
) -> tuple[int, int]:
    samples = 0
    absolute_error = 0
    for y in range(height):
        row = y * width
        for x in range(width):
            pixel = row + x
            if not candidate_mask[pixel]:
                continue
            candidate_offset = pixel * 3
            best: int | None = None
            for reference_y in range(max(0, y - 1), min(height, y + 2)):
                reference_row = reference_y * width
                for reference_x in range(max(0, x - 1), min(width, x + 2)):
                    reference_pixel = reference_row + reference_x
                    if not reference_mask[reference_pixel]:
                        continue
                    reference_offset = reference_pixel * 3
                    error = sum(
                        abs(
                            candidate_rgb[candidate_offset + channel]
                            - reference_rgb[reference_offset + channel]
                        )
                        for channel in range(3)
                    )
                    if best is None or error < best:
                        best = error
            if best is not None:
                samples += 1
                absolute_error += best
    return samples, absolute_error


def image_metrics_python(
    reference_rgb: bytes,
    candidate_rgb: bytes,
    width: int,
    height: int,
    *,
    max_metric_work_units: int | None = None,
) -> dict[str, int]:
    pixels, work_units = _validate_buffers(
        reference_rgb,
        candidate_rgb,
        width,
        height,
        max_metric_work_units,
    )
    evidence = {"pages": 1, **_base_metrics(reference_rgb, candidate_rgb)}
    reference_luma, reference_foreground = _luma_and_foreground(reference_rgb)
    candidate_luma, candidate_foreground = _luma_and_foreground(candidate_rgb)
    evidence.update(
        _mask_evidence(
            "foreground",
            reference_foreground,
            candidate_foreground,
            width,
            height,
        )
    )
    color_samples, color_absolute = _matched_foreground_color_error(
        reference_rgb,
        candidate_rgb,
        reference_foreground,
        candidate_foreground,
        width,
        height,
    )
    color_mae = _ratio_ppm(color_absolute, color_samples * 3 * 255)
    evidence.update(
        {
            "foreground_matched_color_samples": color_samples,
            "foreground_matched_color_absolute_error_sum": color_absolute,
            "foreground_matched_color_mean_absolute_error_ppm": color_mae,
            "foreground_matched_color_similarity_ppm": max(0, PPM - color_mae),
        }
    )
    evidence.update(
        _mask_evidence(
            "edge",
            _edge_mask(reference_luma, width, height),
            _edge_mask(candidate_luma, width, height),
            width,
            height,
        )
    )
    evidence.update(
        _mask_evidence(
            "text_ink",
            _text_ink_mask(reference_luma, width, height),
            _text_ink_mask(candidate_luma, width, height),
            width,
            height,
        )
    )
    reference_blurred = _box_blur_luma_3px(reference_luma, width, height)
    candidate_blurred = _box_blur_luma_3px(candidate_luma, width, height)
    blurred_absolute = sum(
        abs(reference_value - candidate_value)
        for reference_value, candidate_value in zip(
            reference_blurred, candidate_blurred
        )
    )
    blurred_mae = _ratio_ppm(blurred_absolute, pixels * 255)
    evidence.update(
        {
            "blurred_luma_absolute_error_sum": blurred_absolute,
            "blurred_luma_mean_absolute_error_ppm": blurred_mae,
            "blurred_luma_similarity_ppm": max(0, PPM - blurred_mae),
            "metric_work_units": work_units,
        }
    )
    validate_metrics(evidence)
    return evidence


def _numpy_dilate_mask_1px(mask: Any) -> Any:
    expanded = mask.copy()
    expanded[:, 1:] |= mask[:, :-1]
    expanded[:, :-1] |= mask[:, 1:]
    expanded[1:, :] |= mask[:-1, :]
    expanded[:-1, :] |= mask[1:, :]
    expanded[1:, 1:] |= mask[:-1, :-1]
    expanded[1:, :-1] |= mask[:-1, 1:]
    expanded[:-1, 1:] |= mask[1:, :-1]
    expanded[:-1, :-1] |= mask[1:, 1:]
    return expanded


def _numpy_mask_evidence(
    prefix: str, reference_mask: Any, candidate_mask: Any, np: Any
) -> dict[str, int]:
    reference_pixels = int(np.count_nonzero(reference_mask))
    candidate_pixels = int(np.count_nonzero(candidate_mask))
    candidate_matched = int(
        np.count_nonzero(candidate_mask & _numpy_dilate_mask_1px(reference_mask))
    )
    reference_matched = int(
        np.count_nonzero(reference_mask & _numpy_dilate_mask_1px(candidate_mask))
    )
    return _prf_evidence(
        prefix,
        candidate_pixels,
        reference_pixels,
        candidate_matched,
        reference_matched,
    )


def _numpy_luma_and_foreground(rgb: Any, np: Any) -> tuple[Any, Any]:
    channels = rgb.astype(np.uint16)
    luma = (
        77 * channels[:, :, 0]
        + 150 * channels[:, :, 1]
        + 29 * channels[:, :, 2]
        + 128
    ) >> 8
    foreground = np.any(rgb < FOREGROUND_CHANNEL_THRESHOLD, axis=2)
    return luma.astype(np.uint8), foreground


def _numpy_edge_mask(luma: Any, np: Any) -> Any:
    values = luma.astype(np.int16)
    mask = np.zeros(luma.shape, dtype=np.bool_)
    horizontal = np.abs(values[:, 1:] - values[:, :-1]) >= EDGE_LUMA_DELTA
    vertical = np.abs(values[1:, :] - values[:-1, :]) >= EDGE_LUMA_DELTA
    mask[:, 1:] |= horizontal
    mask[:, :-1] |= horizontal
    mask[1:, :] |= vertical
    mask[:-1, :] |= vertical
    return mask


def _numpy_text_ink_mask(luma: Any, np: Any) -> Any:
    values = luma.astype(np.int16)
    mask = np.zeros(luma.shape, dtype=np.bool_)
    mask[:, :-1] |= values[:, 1:] - values[:, :-1] >= EDGE_LUMA_DELTA
    mask[:, 1:] |= values[:, :-1] - values[:, 1:] >= EDGE_LUMA_DELTA
    mask[:-1, :] |= values[1:, :] - values[:-1, :] >= EDGE_LUMA_DELTA
    mask[1:, :] |= values[:-1, :] - values[1:, :] >= EDGE_LUMA_DELTA
    mask &= luma <= TEXT_INK_MAX_LUMA
    return mask


def _numpy_box_blur_luma_3px(luma: Any, np: Any) -> Any:
    values = luma.astype(np.uint16)
    horizontal_sum = values.copy()
    horizontal_sum[:, 1:] += values[:, :-1]
    horizontal_sum[:, :-1] += values[:, 1:]
    horizontal_count = np.full(values.shape[1], 3, dtype=np.uint16)
    horizontal_count[0] -= 1
    horizontal_count[-1] -= 1
    horizontal = (
        horizontal_sum + horizontal_count[np.newaxis, :] // 2
    ) // horizontal_count[np.newaxis, :]

    vertical_sum = horizontal.copy()
    vertical_sum[1:, :] += horizontal[:-1, :]
    vertical_sum[:-1, :] += horizontal[1:, :]
    vertical_count = np.full(values.shape[0], 3, dtype=np.uint16)
    vertical_count[0] -= 1
    vertical_count[-1] -= 1
    return (
        (vertical_sum + vertical_count[:, np.newaxis] // 2)
        // vertical_count[:, np.newaxis]
    ).astype(np.uint8)


def _numpy_matched_foreground_color_error(
    reference_rgb: Any,
    candidate_rgb: Any,
    reference_mask: Any,
    candidate_mask: Any,
    np: Any,
) -> tuple[int, int]:
    height, width = candidate_mask.shape
    sentinel = 1024
    best = np.full((height, width), sentinel, dtype=np.uint16)
    for delta_y in (-1, 0, 1):
        if delta_y < 0:
            candidate_y = slice(1, height)
            reference_y = slice(0, height - 1)
        elif delta_y > 0:
            candidate_y = slice(0, height - 1)
            reference_y = slice(1, height)
        else:
            candidate_y = slice(0, height)
            reference_y = slice(0, height)
        for delta_x in (-1, 0, 1):
            if delta_x < 0:
                candidate_x = slice(1, width)
                reference_x = slice(0, width - 1)
            elif delta_x > 0:
                candidate_x = slice(0, width - 1)
                reference_x = slice(1, width)
            else:
                candidate_x = slice(0, width)
                reference_x = slice(0, width)
            reference_present = reference_mask[reference_y, reference_x]
            error = np.abs(
                np.subtract(
                    candidate_rgb[candidate_y, candidate_x, :],
                    reference_rgb[reference_y, reference_x, :],
                    dtype=np.int16,
                )
            ).sum(axis=2, dtype=np.uint16)
            target = best[candidate_y, candidate_x]
            np.minimum(
                target,
                np.where(reference_present, error, sentinel),
                out=target,
            )
    matched = candidate_mask & (best != sentinel)
    return int(np.count_nonzero(matched)), int(best[matched].sum(dtype=np.uint64))


def image_metrics_numpy(
    reference_rgb: bytes,
    candidate_rgb: bytes,
    width: int,
    height: int,
    *,
    max_metric_work_units: int | None = None,
) -> dict[str, int]:
    np = numpy_module()
    if np is None:
        raise ValueError("NumPy is unavailable")
    pixels, work_units = _validate_buffers(
        reference_rgb,
        candidate_rgb,
        width,
        height,
        max_metric_work_units,
    )
    reference = np.frombuffer(reference_rgb, dtype=np.uint8).reshape(
        height, width, 3
    )
    candidate = np.frombuffer(candidate_rgb, dtype=np.uint8).reshape(
        height, width, 3
    )
    delta = np.abs(np.subtract(reference, candidate, dtype=np.int16))
    changed_pixels = int(np.count_nonzero(np.any(delta, axis=2)))
    absolute_error_sum = int(delta.sum(dtype=np.uint64))
    squared_error_sum = int(
        np.square(delta.astype(np.uint32)).sum(dtype=np.uint64)
    )
    max_channel_delta = int(delta.max())
    channel_denominator = pixels * 3 * 255
    squared_denominator = pixels * 3 * 255 * 255
    mean_absolute_error_ppm = _ratio_ppm(
        absolute_error_sum, channel_denominator
    )
    evidence = {
        "pages": 1,
        "pixels": pixels,
        "changed_pixels": changed_pixels,
        "mismatch_ppm": _ratio_ppm(changed_pixels, pixels),
        "absolute_error_sum": absolute_error_sum,
        "squared_error_sum": squared_error_sum,
        "max_channel_delta": max_channel_delta,
        "mean_absolute_error_ppm": mean_absolute_error_ppm,
        "root_mean_square_error_ppm": math.isqrt(
            (squared_error_sum * PPM * PPM) // squared_denominator
        ),
        "similarity_ppm": max(0, PPM - mean_absolute_error_ppm),
    }

    reference_luma, reference_foreground = _numpy_luma_and_foreground(reference, np)
    candidate_luma, candidate_foreground = _numpy_luma_and_foreground(candidate, np)
    evidence.update(
        _numpy_mask_evidence(
            "foreground", reference_foreground, candidate_foreground, np
        )
    )
    color_samples, color_absolute = _numpy_matched_foreground_color_error(
        reference,
        candidate,
        reference_foreground,
        candidate_foreground,
        np,
    )
    color_mae = _ratio_ppm(color_absolute, color_samples * 3 * 255)
    evidence.update(
        {
            "foreground_matched_color_samples": color_samples,
            "foreground_matched_color_absolute_error_sum": color_absolute,
            "foreground_matched_color_mean_absolute_error_ppm": color_mae,
            "foreground_matched_color_similarity_ppm": max(0, PPM - color_mae),
        }
    )
    evidence.update(
        _numpy_mask_evidence(
            "edge",
            _numpy_edge_mask(reference_luma, np),
            _numpy_edge_mask(candidate_luma, np),
            np,
        )
    )
    evidence.update(
        _numpy_mask_evidence(
            "text_ink",
            _numpy_text_ink_mask(reference_luma, np),
            _numpy_text_ink_mask(candidate_luma, np),
            np,
        )
    )
    reference_blurred = _numpy_box_blur_luma_3px(reference_luma, np)
    candidate_blurred = _numpy_box_blur_luma_3px(candidate_luma, np)
    blurred_absolute = int(
        np.abs(
            np.subtract(reference_blurred, candidate_blurred, dtype=np.int16)
        ).sum(dtype=np.uint64)
    )
    blurred_mae = _ratio_ppm(blurred_absolute, pixels * 255)
    evidence.update(
        {
            "blurred_luma_absolute_error_sum": blurred_absolute,
            "blurred_luma_mean_absolute_error_ppm": blurred_mae,
            "blurred_luma_similarity_ppm": max(0, PPM - blurred_mae),
            "metric_work_units": work_units,
        }
    )
    validate_metrics(evidence)
    return evidence


def image_metrics(
    reference_rgb: bytes,
    candidate_rgb: bytes,
    width: int,
    height: int,
    *,
    max_metric_work_units: int | None = None,
) -> dict[str, int]:
    implementation = image_metrics_numpy if numpy_module() is not None else image_metrics_python
    return implementation(
        reference_rgb,
        candidate_rgb,
        width,
        height,
        max_metric_work_units=max_metric_work_units,
    )


def aggregate_metrics(rows: Sequence[dict[str, int]]) -> dict[str, int]:
    if not rows:
        raise ValueError("metric aggregation requires at least one row")
    for row in rows:
        validate_metrics(row)
    pages = sum(row["pages"] for row in rows)
    pixels = sum(row["pixels"] for row in rows)
    changed_pixels = sum(row["changed_pixels"] for row in rows)
    absolute_error_sum = sum(row["absolute_error_sum"] for row in rows)
    squared_error_sum = sum(row["squared_error_sum"] for row in rows)
    channel_denominator = pixels * 3 * 255
    squared_denominator = pixels * 3 * 255 * 255
    mean_absolute_error_ppm = _ratio_ppm(
        absolute_error_sum, channel_denominator
    )
    evidence = {
        "pages": pages,
        "pixels": pixels,
        "changed_pixels": changed_pixels,
        "mismatch_ppm": _ratio_ppm(changed_pixels, pixels),
        "absolute_error_sum": absolute_error_sum,
        "squared_error_sum": squared_error_sum,
        "max_channel_delta": max(row["max_channel_delta"] for row in rows),
        "mean_absolute_error_ppm": mean_absolute_error_ppm,
        "root_mean_square_error_ppm": math.isqrt(
            (squared_error_sum * PPM * PPM) // squared_denominator
        ),
        "similarity_ppm": max(0, PPM - mean_absolute_error_ppm),
        "metric_work_units": sum(row["metric_work_units"] for row in rows),
    }
    for prefix in MASK_PREFIXES:
        evidence.update(
            _prf_evidence(
                prefix,
                sum(row[f"{prefix}_candidate_pixels"] for row in rows),
                sum(row[f"{prefix}_reference_pixels"] for row in rows),
                sum(row[f"{prefix}_candidate_matched_1px"] for row in rows),
                sum(row[f"{prefix}_reference_matched_1px"] for row in rows),
            )
        )
    color_samples = sum(
        row["foreground_matched_color_samples"] for row in rows
    )
    color_absolute = sum(
        row["foreground_matched_color_absolute_error_sum"] for row in rows
    )
    color_mae = _ratio_ppm(color_absolute, color_samples * 3 * 255)
    blurred_absolute = sum(
        row["blurred_luma_absolute_error_sum"] for row in rows
    )
    blurred_mae = _ratio_ppm(blurred_absolute, pixels * 255)
    evidence.update(
        {
            "foreground_matched_color_samples": color_samples,
            "foreground_matched_color_absolute_error_sum": color_absolute,
            "foreground_matched_color_mean_absolute_error_ppm": color_mae,
            "foreground_matched_color_similarity_ppm": max(0, PPM - color_mae),
            "blurred_luma_absolute_error_sum": blurred_absolute,
            "blurred_luma_mean_absolute_error_ppm": blurred_mae,
            "blurred_luma_similarity_ppm": max(0, PPM - blurred_mae),
        }
    )
    validate_metrics(evidence)
    return evidence


def validate_metrics(evidence: object) -> None:
    if not isinstance(evidence, dict) or set(evidence) != METRIC_KEYS:
        raise ValueError("integer visual metric keys are invalid")
    for key, value in evidence.items():
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise ValueError(f"integer visual metric is invalid: {key}")
    if evidence["pages"] == 0 or evidence["pixels"] == 0:
        raise ValueError("integer visual metrics must contain pages and pixels")
    ppm_keys = {
        key
        for key in METRIC_KEYS
        if key.endswith("_ppm")
    }
    for key in ppm_keys:
        if evidence[key] > PPM:
            raise ValueError(f"integer visual metric is out of range: {key}")
    if evidence["changed_pixels"] > evidence["pixels"]:
        raise ValueError("integer visual changed pixel count is invalid")
    if evidence["max_channel_delta"] > 255:
        raise ValueError("integer visual max channel delta is invalid")
    if evidence["absolute_error_sum"] > evidence["pixels"] * 3 * 255:
        raise ValueError("integer visual absolute error is invalid")
    if evidence["squared_error_sum"] > evidence["pixels"] * 3 * 255 * 255:
        raise ValueError("integer visual squared error is invalid")
    expected_base = {
        "mismatch_ppm": _ratio_ppm(
            evidence["changed_pixels"], evidence["pixels"]
        ),
        "mean_absolute_error_ppm": _ratio_ppm(
            evidence["absolute_error_sum"], evidence["pixels"] * 3 * 255
        ),
        "root_mean_square_error_ppm": math.isqrt(
            (evidence["squared_error_sum"] * PPM * PPM)
            // (evidence["pixels"] * 3 * 255 * 255)
        ),
        "metric_work_units": evidence["pixels"] * METRIC_WORK_UNITS_PER_PIXEL,
    }
    expected_base["similarity_ppm"] = max(
        0, PPM - expected_base["mean_absolute_error_ppm"]
    )
    for key, expected in expected_base.items():
        if evidence[key] != expected:
            raise ValueError(f"integer visual metric is inconsistent: {key}")
    for prefix in MASK_PREFIXES:
        candidate_pixels = evidence[f"{prefix}_candidate_pixels"]
        reference_pixels = evidence[f"{prefix}_reference_pixels"]
        candidate_matched = evidence[f"{prefix}_candidate_matched_1px"]
        reference_matched = evidence[f"{prefix}_reference_matched_1px"]
        if candidate_pixels > evidence["pixels"] or reference_pixels > evidence["pixels"]:
            raise ValueError(f"integer visual mask count is invalid: {prefix}")
        if candidate_matched > candidate_pixels or reference_matched > reference_pixels:
            raise ValueError(f"integer visual matched count is invalid: {prefix}")
        expected = _prf_evidence(
            prefix,
            candidate_pixels,
            reference_pixels,
            candidate_matched,
            reference_matched,
        )
        for key, value in expected.items():
            if evidence[key] != value:
                raise ValueError(f"integer visual metric is inconsistent: {key}")
    color_samples = evidence["foreground_matched_color_samples"]
    color_absolute = evidence["foreground_matched_color_absolute_error_sum"]
    if color_samples > evidence["foreground_candidate_pixels"]:
        raise ValueError("integer visual color sample count is invalid")
    if color_absolute > color_samples * 3 * 255:
        raise ValueError("integer visual color error is invalid")
    color_mae = _ratio_ppm(color_absolute, color_samples * 3 * 255)
    if evidence["foreground_matched_color_mean_absolute_error_ppm"] != color_mae:
        raise ValueError("integer visual color mean error is inconsistent")
    if evidence["foreground_matched_color_similarity_ppm"] != max(0, PPM - color_mae):
        raise ValueError("integer visual color similarity is inconsistent")
    blurred_absolute = evidence["blurred_luma_absolute_error_sum"]
    if blurred_absolute > evidence["pixels"] * 255:
        raise ValueError("integer visual blurred luma error is invalid")
    blurred_mae = _ratio_ppm(blurred_absolute, evidence["pixels"] * 255)
    if evidence["blurred_luma_mean_absolute_error_ppm"] != blurred_mae:
        raise ValueError("integer visual blurred luma mean error is inconsistent")
    if evidence["blurred_luma_similarity_ppm"] != max(0, PPM - blurred_mae):
        raise ValueError("integer visual blurred luma similarity is inconsistent")
