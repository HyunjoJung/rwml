#!/usr/bin/env python3
"""Attest Type 1 or mapped CFF subsets against locked source glyphs in a container."""

from __future__ import annotations

import argparse
from fractions import Fraction
import json
import math
from pathlib import Path
import re
import sys
import tempfile
import uuid

import font_subset_worker as worker
import libreoffice_container as runtime
from libreoffice_oracle_fonts import _positive_int, _safe_text, _sha256
from render_oracle_contract import _assert_path_neutral, _load_json, _require_exact_keys
import shared_oracle_fonts as shared

TOOL_LOCK = runtime.ROOT / "corpus/public/oracle/fonttools-lock.json"
WORKER = Path(__file__).with_name("font_subset_worker.py")
SCRATCH = runtime.ROOT / "target/render-oracle/font-subsets"
WHEEL_URL = "https://files.pythonhosted.org/packages/2c/47/c99d5268f354002ce80f8d029cd9d7d872969da1de8b93d32de4dc56d6f4/fonttools-4.63.0-py3-none-any.whl"


def tool_lock() -> dict:
    actual, _ = _load_json(TOOL_LOCK, 65536)
    expected = {
        "schema": "rwml.fonttools-lock.v1",
        "version": worker.WHEEL_VERSION,
        "wheel": {
            "name": "fonttools-4.63.0-py3-none-any.whl",
            "bytes": worker.WHEEL_BYTES,
            "sha256": worker.WHEEL_SHA256,
            "url": WHEEL_URL,
        },
    }
    if worker.canonical(actual) != worker.canonical(expected):
        raise ValueError("FontTools lock differs")
    return actual


def wheel_payload(path: Path) -> bytes:
    tool_lock()
    payload = runtime.read_regular_file(path, worker.WHEEL_BYTES)
    if (
        len(payload) != worker.WHEEL_BYTES
        or worker.digest(payload) != worker.WHEEL_SHA256
    ):
        raise ValueError("FontTools wheel identity differs")
    return payload


def worker_command(image: str, name: str, directory: Path) -> list[str]:
    command = runtime.create_command(image, name, directory, directory)
    return [
        *command[:-1],
        "--entrypoint",
        "/opt/libreoffice26.2/program/python",
        image,
        "-B",
        "-s",
        "-S",
        "-P",
        "/oracle/source/worker.py",
    ]


def rational(value: object) -> tuple[int, int]:
    if (
        not isinstance(value, list)
        or len(value) != 2
        or any(type(number) is not int or number.bit_length() > 96 for number in value)
        or value[1] <= 0
    ):
        raise ValueError("proof coordinate is invalid")
    normalized = Fraction(*value)
    if (
        abs(normalized) > 1048576
        or [normalized.numerator, normalized.denominator] != value
    ):
        raise ValueError("proof coordinate is not canonical")
    return tuple(value)


def validate_result(result: dict, request: dict) -> None:
    if not isinstance(result, dict):
        raise ValueError("worker result must be an object")
    expected = {
        "schema": "rwml.font-subset-worker.v1",
        "source": request["source"],
        "subset": request["subset"],
        "worker_sha256": request["worker_sha256"],
        "fonttools_version": worker.WHEEL_VERSION,
        "wheel_sha256": worker.WHEEL_SHA256,
        "python": worker.PYTHON_VERSION,
        "limits": worker.LIMITS,
    }
    _require_exact_keys(result, set(expected) | {"proof"}, "worker result")
    if worker.canonical({key: result[key] for key in expected}) != worker.canonical(
        expected
    ):
        raise ValueError("worker result identity differs")
    proof = result["proof"]
    if not isinstance(proof, dict):
        raise ValueError("worker proof must be an object")
    _require_exact_keys(
        proof, {"glyph_count", "glyphs", "matrix", "outline_sha256"}, "worker proof"
    )
    count = _positive_int(
        proof["glyph_count"], "proof glyph count", worker.LIMITS["max_glyphs"]
    )
    rows = proof["glyphs"]
    if not isinstance(rows, list) or len(rows) != count or count < 2:
        raise ValueError("proof glyph count differs")
    if not isinstance(proof["matrix"], list) or len(proof["matrix"]) != 6:
        raise ValueError("proof matrix differs")
    for value in proof["matrix"]:
        rational(value)
    subset, source = [], []
    for row in rows:
        if not isinstance(row, dict):
            raise ValueError("proof glyph must be an object")
        _require_exact_keys(
            row, {"subset", "source", "width", "outline_sha256"}, "proof glyph"
        )
        _safe_text(row["subset"], "subset glyph", 32)
        _safe_text(row["source"], "source glyph", 32)
        subset.append(row["subset"])
        source.append(row["source"])
        rational(row["width"])
        _sha256(row["outline_sha256"], "glyph outline digest")
    if (
        len(set(subset)) != count
        or len(set(source)) != count
        or subset != sorted(subset)
    ):
        raise ValueError("proof glyph identity differs")
    if request["subset"]["representation"] == "cid-cff":
        pairs = worker.cff_glyph_mapping(
            dict.fromkeys(source), dict.fromkeys(subset), request["subset"]["glyph_map"]
        )
    else:
        pairs = worker.glyph_mapping(dict.fromkeys(source), dict.fromkeys(subset))
    if pairs != list(zip(subset, source)):
        raise ValueError("proof glyph mapping differs")
    identity = {"matrix": proof["matrix"], "glyphs": rows}
    if proof["outline_sha256"] != worker.digest(worker.canonical(identity)):
        raise ValueError("proof aggregate digest differs")
    _assert_path_neutral(result)


def attest_program(
    program: bytes,
    source: bytes,
    entry: dict,
    wheel: Path,
    *,
    timeout: float = 30,
    glyph_map: list | None = None,
) -> dict:
    if not math.isfinite(timeout) or not 0 < timeout <= 30:
        raise ValueError("font attestation timeout is outside its bound")
    if glyph_map is not None:
        worker.validate_cff_map(glyph_map)
    prefix = b"%!FontType1-" if glyph_map is None else b"\x01\x00"
    if not 0 < len(program) <= worker.MAX_SUBSET_BYTES or not program.startswith(
        prefix
    ):
        raise ValueError(
            "font subset is not bounded Type 1/PFA or explicitly mapped CFF"
        )
    _positive_int(entry["bytes"], "source bytes", worker.MAX_SOURCE_BYTES)
    _positive_int(entry["sfnt_revision"], "source revision", 0xFFFFFFFF)
    name = entry["postscript_name"]
    if (
        not isinstance(name, str)
        or re.fullmatch(r"[A-Za-z0-9_.-]{1,127}", name) is None
    ):
        raise ValueError("source PostScript name is invalid")
    if (
        len(source) != entry["bytes"]
        or worker.digest(source) != entry["sha256"]
        or source[:4] != b"OTTO"
    ):
        raise ValueError("CJK source identity differs")
    wheel_bytes = wheel_payload(wheel)
    code = runtime.read_regular_file(WORKER, 1024 * 1024)
    lock = runtime.load_runtime_lock()
    image = runtime.inspect_image(lock)
    request = {
        "schema": "rwml.font-subset-request.v1",
        "source": {
            "bytes": len(source),
            "sha256": worker.digest(source),
            "postscript_name": name,
            "sfnt_revision": entry["sfnt_revision"],
        },
        "subset": {
            "bytes": len(program),
            "sha256": worker.digest(program),
            "representation": "type1-pfa" if glyph_map is None else "cid-cff",
        },
        "worker_sha256": worker.digest(code),
    }
    if glyph_map is not None:
        request["subset"]["glyph_map"] = glyph_map
    worker.validate_request(request)
    files = {
        "source.otf": source,
        "subset.bin": program,
        "fonttools.whl": wheel_bytes,
        "worker.py": code,
        "request.json": worker.canonical(request),
    }
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="capture-", dir=SCRATCH) as temporary:
        directory = Path(temporary)
        for filename, payload in files.items():
            with (directory / filename).open("xb") as stream:
                stream.write(payload)
        container_name = "rwml-oracle-" + uuid.uuid4().hex
        payload = runtime.run_container(
            worker_command(image, container_name, directory),
            container_name,
            timeout=timeout,
            stdout_limit=worker.MAX_RESULT_BYTES,
        )
        for filename, expected in files.items():
            if (
                runtime.read_regular_file(directory / filename, len(expected))
                != expected
            ):
                raise ValueError("font worker input changed")
    result = worker.strict_json(payload)
    validate_result(result, request)
    if (
        runtime.read_regular_file(WORKER, 1024 * 1024) != code
        or runtime.load_runtime_lock() != lock
        or runtime.inspect_image(lock) != image
        or wheel_payload(wheel) != wheel_bytes
    ):
        raise ValueError("font worker environment changed")
    return {
        "schema": "rwml.font-subset-attestation.v1",
        "runtime_lock_sha256": worker.digest(worker.canonical(lock)),
        "image_manifest_sha256": lock["image"]["manifest_sha256"],
        "tool_lock_sha256": worker.digest(worker.canonical(tool_lock())),
        "result": result,
    }


def verify_receipt(payload: bytes, recomputed: dict) -> None:
    if not 0 < len(payload) <= worker.MAX_RESULT_BYTES:
        raise ValueError("receipt size exceeds its bound")
    if worker.canonical(worker.strict_json(payload)) != worker.canonical(recomputed):
        raise ValueError("receipt differs from independently recomputed proof")


def load_cff_map(payload: bytes, source: bytes, subset: bytes) -> list:
    if not 0 < len(payload) <= 65536:
        raise ValueError("CFF map size exceeds its bound")
    value = worker.strict_json(payload)
    if not isinstance(value, dict):
        raise ValueError("CFF map must be an object")
    _require_exact_keys(
        value, {"schema", "source_sha256", "subset_sha256", "glyphs"}, "CFF map"
    )
    if (
        value["schema"] != "rwml.cff-glyph-map.v1"
        or value["source_sha256"] != worker.digest(source)
        or value["subset_sha256"] != worker.digest(subset)
    ):
        raise ValueError("CFF map input identity differs")
    worker.validate_cff_map(value["glyphs"])
    return value["glyphs"]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--font-pack", type=Path, required=True)
    parser.add_argument("--fonttools-wheel", type=Path, required=True)
    parser.add_argument("--program", type=Path, required=True)
    parser.add_argument("--cff-glyph-map", type=Path)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path)
    mode.add_argument("--verify", type=Path)
    args = parser.parse_args()
    try:
        if args.output and (args.output.exists() or args.output.is_symlink()):
            raise ValueError("output must be fresh")
        retained = None
        if args.verify:
            retained = runtime.read_regular_file(args.verify, worker.MAX_RESULT_BYTES)
            worker.strict_json(retained)
        lock = shared.load_lock()
        shared.verify_pack(args.font_pack, lock)
        entry = next(
            entry
            for entry in lock.fonts
            if entry["name"] == "NotoSansCJKkr-Regular.otf"
        )
        source = runtime.read_regular_file(
            args.font_pack / "fonts" / entry["name"], entry["bytes"]
        )
        program = runtime.read_regular_file(args.program, worker.MAX_SUBSET_BYTES)
        options = {}
        mapping_bytes = None
        if args.cff_glyph_map:
            mapping_bytes = runtime.read_regular_file(args.cff_glyph_map, 65536)
            options["glyph_map"] = load_cff_map(mapping_bytes, source, program)
        result = attest_program(program, source, entry, args.fonttools_wheel, **options)
        shared.verify_pack(args.font_pack, lock)
        if runtime.read_regular_file(args.program, worker.MAX_SUBSET_BYTES) != program:
            raise ValueError("subset input changed")
        if (
            args.cff_glyph_map
            and runtime.read_regular_file(args.cff_glyph_map, 65536) != mapping_bytes
        ):
            raise ValueError("CFF map input changed")
        if args.verify:
            verify_receipt(retained, result)
            if (
                runtime.read_regular_file(args.verify, worker.MAX_RESULT_BYTES)
                != retained
            ):
                raise ValueError("retained receipt changed")
        else:
            with args.output.open("xb") as stream:
                stream.write(worker.canonical(result) + b"\n")
        print(
            json.dumps(
                {
                    "glyphs": result["result"]["proof"]["glyph_count"],
                    "outline_sha256": result["result"]["proof"]["outline_sha256"],
                },
                sort_keys=True,
            )
        )
        return 0
    except (OSError, ValueError, StopIteration) as error:
        print(f"font_subset_attestation: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
