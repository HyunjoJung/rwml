#!/usr/bin/env python3
"""Bounded Type 1 glyph comparison; parsing runs only in the locked Linux worker."""

from __future__ import annotations

from fractions import Fraction
import hashlib
import io
import json
import math
from pathlib import Path
import re
import sys

WHEEL_VERSION = "4.63.0"
WHEEL_BYTES = 1164562
WHEEL_SHA256 = "445af2eab030a16b9171ea8bdda7ebf7d96bda2df88ee182a464252f6e05e20d"
PYTHON_VERSION = "3.12.13"
MAX_SOURCE_BYTES = 64 * 1024 * 1024
MAX_SUBSET_BYTES = 4 * 1024 * 1024
MAX_RESULT_BYTES = 512 * 1024
LIMITS = {
    "container_memory_bytes": 2 * 1024 * 1024 * 1024,
    "data_bytes": 512 * 1024 * 1024,
    "cpu_seconds": 20,
    "max_glyphs": 1024,
    "max_commands": 131072,
    "max_commands_per_glyph": 8192,
}


class SubsetError(ValueError):
    pass


def canonical(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode()


def digest(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def number(value: object) -> tuple[int, int]:
    if (
        type(value) not in (int, float)
        or abs(value) > 1048576
        or not math.isfinite(value)
    ):
        raise SubsetError("coordinate_bound")
    rational = Fraction(value)
    if rational.numerator.bit_length() > 96 or rational.denominator.bit_length() > 96:
        raise SubsetError("coordinate_precision")
    return rational.numerator, rational.denominator


class Budget:
    def __init__(self, limit: int):
        self.remaining = limit


class BoundedPen:
    def __init__(
        self, budget: Budget, glyph_limit: int = LIMITS["max_commands_per_glyph"]
    ):
        self.budget = budget
        self.limit = glyph_limit
        self.commands = []

    def record(self, operation: str, points: tuple) -> None:
        self.budget.remaining -= 1
        if self.budget.remaining < 0 or len(self.commands) >= self.limit:
            raise SubsetError("outline_work_bound")
        coordinates = []
        for point in points:
            if not isinstance(point, (tuple, list)) or len(point) != 2:
                raise SubsetError("outline_point")
            coordinates.append(tuple(number(value) for value in point))
        self.commands.append((operation, tuple(coordinates)))

    def moveTo(self, point):
        self.record("moveTo", (point,))

    def lineTo(self, point):
        self.record("lineTo", (point,))

    def curveTo(self, *points):
        if len(points) != 3:
            raise SubsetError("outline_curve")
        self.record("curveTo", points)

    def closePath(self):
        self.record("closePath", ())

    def endPath(self):
        self.record("endPath", ())

    def qCurveTo(self, *points):
        raise SubsetError("outline_quadratic_unsupported")

    def addComponent(self, *args, **kwargs):
        raise SubsetError("outline_component_unsupported")

    def addVarComponent(self, *args, **kwargs):
        raise SubsetError("outline_component_unsupported")


def glyph_mapping(source, subset) -> list[tuple[str, str]]:
    if not 2 <= len(subset) <= LIMITS["max_glyphs"] or ".notdef" not in subset:
        raise SubsetError("subset_glyph_count")
    rows = []
    seen = set()
    for name in sorted(subset):
        if name == ".notdef":
            source_name = name
        elif isinstance(name, str) and re.fullmatch(r"cid[0-9]{1,5}", name):
            source_name = f"cid{int(name[3:]):05d}"
        else:
            raise SubsetError("subset_glyph_name")
        if source_name in seen or source_name not in source:
            raise SubsetError("subset_glyph_mapping")
        seen.add(source_name)
        rows.append((name, source_name))
    return rows


def compare_glyphs(
    source,
    subset,
    source_matrix,
    subset_matrix,
    *,
    command_limit=LIMITS["max_commands"],
):
    if (
        not isinstance(source_matrix, (list, tuple))
        or not isinstance(subset_matrix, (list, tuple))
        or len(source_matrix) != 6
        or len(subset_matrix) != 6
    ):
        raise SubsetError("font_matrix")
    matrix = tuple(number(value) for value in source_matrix)
    if matrix != tuple(number(value) for value in subset_matrix):
        raise SubsetError("font_matrix_mismatch")
    budget = Budget(command_limit)
    rows = []
    for name, source_name in glyph_mapping(source, subset):
        source_pen, subset_pen = BoundedPen(budget), BoundedPen(budget)
        source[source_name].draw(source_pen)
        subset[name].draw(subset_pen)
        width = number(source[source_name].width)
        if width != number(subset[name].width):
            raise SubsetError("glyph_width_mismatch")
        if source_pen.commands != subset_pen.commands:
            raise SubsetError("glyph_outline_mismatch")
        rows.append(
            {
                "subset": name,
                "source": source_name,
                "width": width,
                "outline_sha256": digest(canonical(source_pen.commands)),
            }
        )
    identity = {"matrix": matrix, "glyphs": rows}
    return {
        **identity,
        "glyph_count": len(rows),
        "outline_sha256": digest(canonical(identity)),
    }


def resource_limits() -> None:
    import resource

    if (
        sys.platform != "linux"
        or ".".join(map(str, sys.version_info[:3])) != PYTHON_VERSION
    ):
        raise SubsetError("worker_runtime")
    cgroup = Path("/sys/fs/cgroup")
    if (cgroup / "memory.max").read_text().strip() != str(
        LIMITS["container_memory_bytes"]
    ) or (cgroup / "memory.swap.max").read_text().strip() != "0":
        raise SubsetError("worker_memory_limit")
    if (cgroup / "pids.max").read_text().strip() != "128" or (
        cgroup / "cpu.max"
    ).read_text().split() != ["200000", "100000"]:
        raise SubsetError("worker_process_limit")
    resource.setrlimit(
        resource.RLIMIT_DATA, (LIMITS["data_bytes"], LIMITS["data_bytes"])
    )
    resource.setrlimit(
        resource.RLIMIT_CPU, (LIMITS["cpu_seconds"], LIMITS["cpu_seconds"] + 1)
    )
    resource.setrlimit(resource.RLIMIT_FSIZE, (8 * 1024 * 1024, 8 * 1024 * 1024))
    resource.setrlimit(resource.RLIMIT_NOFILE, (64, 64))
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    sys.setrecursionlimit(256)


def read_bounded(path: Path, maximum: int) -> bytes:
    import os
    import stat

    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        metadata = os.fstat(descriptor)
        if not stat.S_ISREG(metadata.st_mode) or not 0 < metadata.st_size <= maximum:
            raise SubsetError("input_file_bound")
        with os.fdopen(descriptor, "rb", closefd=False) as stream:
            payload = stream.read(maximum + 1)
        if len(payload) != metadata.st_size:
            raise SubsetError("input_changed")
        return payload
    finally:
        os.close(descriptor)


def strict_json(payload: bytes):
    def pairs(values):
        result = {}
        for key, value in values:
            if key in result:
                raise SubsetError("duplicate_json_key")
            result[key] = value
        return result

    def constant(value):
        raise SubsetError("nonfinite_json")

    return json.loads(payload, object_pairs_hook=pairs, parse_constant=constant)


def validate_request(request: object) -> None:
    if (
        not isinstance(request, dict)
        or set(request) != {"schema", "source", "subset", "worker_sha256"}
        or request["schema"] != "rwml.font-subset-request.v1"
    ):
        raise SubsetError("request_schema")
    for key, fields, maximum in (
        (
            "source",
            {"bytes", "sha256", "postscript_name", "sfnt_revision"},
            MAX_SOURCE_BYTES,
        ),
        ("subset", {"bytes", "sha256", "representation"}, MAX_SUBSET_BYTES),
    ):
        entry = request[key]
        if (
            not isinstance(entry, dict)
            or set(entry) != fields
            or type(entry["bytes"]) is not int
            or not 0 < entry["bytes"] <= maximum
            or not isinstance(entry["sha256"], str)
            or re.fullmatch(r"[0-9a-f]{64}", entry["sha256"]) is None
        ):
            raise SubsetError("request_input_identity")
    source = request["source"]
    if (
        not isinstance(request["worker_sha256"], str)
        or re.fullmatch(r"[0-9a-f]{64}", request["worker_sha256"]) is None
        or not isinstance(source["postscript_name"], str)
        or re.fullmatch(r"[A-Za-z0-9_.-]{1,127}", source["postscript_name"]) is None
        or type(source["sfnt_revision"]) is not int
        or not 0 < source["sfnt_revision"] <= 0xFFFFFFFF
        or request["subset"]["representation"] != "type1-pfa"
    ):
        raise SubsetError("request_font_identity")


def run_worker(directory: Path, output: Path) -> dict:
    resource_limits()
    request = strict_json(read_bounded(directory / "request.json", 65536))
    validate_request(request)
    source = read_bounded(directory / "source.otf", MAX_SOURCE_BYTES)
    program = read_bounded(directory / "subset.pfa", MAX_SUBSET_BYTES)
    wheel = read_bounded(directory / "fonttools.whl", WHEEL_BYTES)
    if len(wheel) != WHEEL_BYTES or digest(wheel) != WHEEL_SHA256:
        raise SubsetError("fonttools_identity")
    if request["worker_sha256"] != digest(read_bounded(Path(__file__), 1024 * 1024)):
        raise SubsetError("worker_identity")
    if not program.startswith(b"%!FontType1-") or source[:4] != b"OTTO":
        raise SubsetError("font_representation")
    for entry, payload in ((request["source"], source), (request["subset"], program)):
        if (
            entry["sha256"] != digest(payload)
            or type(entry["bytes"]) is not int
            or entry["bytes"] != len(payload)
        ):
            raise SubsetError("font_input_identity")
    # Import only a verified in-container snapshot, not a mutable host wheel.
    wheel_path = output / "fonttools.whl"
    subset_path = output / "subset.pfa"
    with wheel_path.open("xb") as stream:
        stream.write(wheel)
    with subset_path.open("xb") as stream:
        stream.write(program)
    python_root = "/opt/libreoffice26.2/program/python-core-3.12.13/lib"
    sys.path = [
        str(wheel_path),
        *[
            path
            for path in sys.path
            if path.startswith(python_root) and "site-packages" not in path
        ],
    ]
    import fontTools
    from fontTools.t1Lib import T1Font
    from fontTools.ttLib import TTFont

    if fontTools.version != WHEEL_VERSION:
        raise SubsetError("fonttools_version")
    font = TTFont(io.BytesIO(source), lazy=True)
    if "CFF " not in font or "fvar" in font or len(font.getGlyphOrder()) > 65536:
        raise SubsetError("source_format")
    name = request["source"]["postscript_name"]
    if (
        font["name"].getDebugName(6) != name
        or round(font["head"].fontRevision * 65536)
        != request["source"]["sfnt_revision"]
    ):
        raise SubsetError("source_font_identity")
    top = font["CFF "].cff.topDictIndex[0]
    if any("FontMatrix" in entry.rawDict for entry in getattr(top, "FDArray", [])):
        raise SubsetError("source_fd_matrix_unsupported")
    subset = T1Font(subset_path, kind="OTHER")
    if (
        re.sub(r"^[A-Z]{6}\+", "", subset["FontName"]) != name
        or subset["FontType"] != 1
        or subset["PaintType"] != 0
    ):
        raise SubsetError("subset_font_identity")
    proof = compare_glyphs(
        font.getGlyphSet(), subset.getGlyphSet(), top.FontMatrix, subset["FontMatrix"]
    )
    font.close()
    return {
        "schema": "rwml.font-subset-worker.v1",
        "source": request["source"],
        "subset": request["subset"],
        "worker_sha256": request["worker_sha256"],
        "fonttools_version": WHEEL_VERSION,
        "wheel_sha256": WHEEL_SHA256,
        "python": PYTHON_VERSION,
        "limits": LIMITS,
        "proof": proof,
    }


def main() -> int:
    try:
        result = run_worker(Path("/oracle/source"), Path("/oracle/output"))
        payload = canonical(result)
        if len(payload) > MAX_RESULT_BYTES:
            raise SubsetError("result_size")
        sys.stdout.buffer.write(payload + b"\n")
        return 0
    except SubsetError as error:
        print(f"font_subset_worker: {error}", file=sys.stderr)
    except Exception:
        print("font_subset_worker: parser_rejected_input", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
