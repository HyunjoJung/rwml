#!/usr/bin/env python3
"""Extract bounded PDF font-resource bytes; this is not a font or layout proof."""

from __future__ import annotations

import argparse
import math
from pathlib import Path
import sys
import tempfile
import uuid

import font_subset_attestation as attestation
import font_subset_worker as common
import libreoffice_container as runtime
import pdf_font_worker as worker
from render_oracle_contract import _load_json, _validate_json_complexity

TOOL_LOCK = runtime.ROOT / "corpus/public/oracle/pypdf-lock.json"
WORKER = Path(__file__).with_name("pdf_font_worker.py")
HELPER = Path(__file__).with_name("font_subset_worker.py")
SCRATCH = runtime.ROOT / "target/render-oracle/pdf-fonts"
WHEEL_URL = "https://files.pythonhosted.org/packages/13/f1/a2da3b55acd4ab737bf728c97edaaed5ec1d3c1236acb639dcdfa97e42c7/pypdf-6.16.2-py3-none-any.whl"


def tool_lock() -> dict:
    actual, _ = _load_json(TOOL_LOCK, 65536)
    expected = {
        "schema": "rwml.pypdf-lock.v1",
        "version": worker.WHEEL_VERSION,
        "wheel": {
            "name": "pypdf-6.16.2-py3-none-any.whl",
            "bytes": worker.WHEEL_BYTES,
            "sha256": worker.WHEEL_SHA256,
            "url": WHEEL_URL,
        },
    }
    if common.canonical(actual) != common.canonical(expected):
        raise ValueError("pypdf lock differs")
    return actual


def wheel_payload(path: Path) -> bytes:
    tool_lock()
    payload = runtime.read_regular_file(path, worker.WHEEL_BYTES)
    if (
        len(payload) != worker.WHEEL_BYTES
        or common.digest(payload) != worker.WHEEL_SHA256
    ):
        raise ValueError("pypdf wheel identity differs")
    return payload


def validate_result(result: object, request: dict) -> None:
    worker.validate_request(request)
    expected = {
        **request,
        "schema": "rwml.pdf-font-worker.v1",
        "parser_version": worker.WHEEL_VERSION,
        "wheel_sha256": worker.WHEEL_SHA256,
        "python": common.PYTHON_VERSION,
        "limits": {**common.LIMITS, **worker.PDF_LIMITS},
    }
    if (
        not isinstance(result, dict)
        or set(result) != set(expected) | {"fonts", "blobs"}
        or common.canonical({key: result[key] for key in expected})
        != common.canonical(expected)
    ):
        raise ValueError("PDF worker result identity differs")
    worker.validate_inventory(result["fonts"], result["blobs"])


def strict_json(payload: bytes):
    if not 0 < len(payload) <= worker.MAX_RESULT_BYTES:
        raise ValueError("PDF JSON size exceeds its bound")
    try:
        value = common.strict_json(payload)
        _validate_json_complexity(value)
        return value
    except RecursionError as error:
        raise ValueError("PDF JSON depth exceeds its bound") from error


def extract_pdf(payload: bytes, wheel: Path, *, timeout: float = 30) -> dict:
    if not math.isfinite(timeout) or not 0 < timeout <= 30:
        raise ValueError("PDF extraction timeout is outside its bound")
    if not 0 < len(payload) <= worker.MAX_PDF_BYTES or not payload.startswith(b"%PDF-"):
        raise ValueError("PDF input is outside its bound")
    wheel_bytes = wheel_payload(wheel)
    code = runtime.read_regular_file(WORKER, 1024 * 1024)
    helper = runtime.read_regular_file(HELPER, 1024 * 1024)
    lock = runtime.load_runtime_lock()
    image = runtime.inspect_image(lock)
    request = {
        "schema": "rwml.pdf-font-request.v1",
        "pdf": {"bytes": len(payload), "sha256": common.digest(payload)},
        "worker_sha256": common.digest(code),
        "helper_sha256": common.digest(helper),
    }
    worker.validate_request(request)
    files = {
        "input.pdf": payload,
        "pypdf.whl": wheel_bytes,
        "worker.py": code,
        "font_subset_worker.py": helper,
        "request.json": common.canonical(request),
    }
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="capture-", dir=SCRATCH) as temporary:
        directory = Path(temporary)
        for filename, data in files.items():
            with (directory / filename).open("xb") as stream:
                stream.write(data)
        name = "rwml-oracle-" + uuid.uuid4().hex
        output = runtime.run_container(
            attestation.worker_command(image, name, directory),
            name,
            timeout=timeout,
            stdout_limit=worker.MAX_RESULT_BYTES,
        )
        for filename, expected in files.items():
            if (
                runtime.read_regular_file(directory / filename, len(expected))
                != expected
            ):
                raise ValueError("PDF worker input changed")
    result = strict_json(output)
    validate_result(result, request)
    if (
        runtime.read_regular_file(WORKER, 1024 * 1024) != code
        or runtime.read_regular_file(HELPER, 1024 * 1024) != helper
        or runtime.load_runtime_lock() != lock
        or runtime.inspect_image(lock) != image
        or wheel_payload(wheel) != wheel_bytes
    ):
        raise ValueError("PDF worker environment changed")
    return {
        "schema": "rwml.pdf-font-extraction.v1",
        "runtime_lock_sha256": common.digest(common.canonical(lock)),
        "image_manifest_sha256": lock["image"]["manifest_sha256"],
        "tool_lock_sha256": common.digest(common.canonical(tool_lock())),
        "result": result,
    }


def verify_receipt(payload: bytes, recomputed: dict) -> None:
    if not 0 < len(payload) <= worker.MAX_RESULT_BYTES:
        raise ValueError("PDF receipt size exceeds its bound")
    if common.canonical(strict_json(payload)) != common.canonical(recomputed):
        raise ValueError("PDF receipt differs from independently recomputed extraction")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--pypdf-wheel", type=Path, required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path)
    mode.add_argument("--verify", type=Path)
    args = parser.parse_args()
    try:
        if args.output and (args.output.exists() or args.output.is_symlink()):
            raise ValueError("output must be fresh")
        retained = (
            runtime.read_regular_file(args.verify, worker.MAX_RESULT_BYTES)
            if args.verify
            else None
        )
        if retained is not None:
            strict_json(retained)
        pdf = runtime.read_regular_file(args.pdf, worker.MAX_PDF_BYTES)
        result = extract_pdf(pdf, args.pypdf_wheel)
        if runtime.read_regular_file(args.pdf, worker.MAX_PDF_BYTES) != pdf:
            raise ValueError("PDF input changed")
        if args.verify:
            verify_receipt(retained, result)
            if (
                runtime.read_regular_file(args.verify, worker.MAX_RESULT_BYTES)
                != retained
            ):
                raise ValueError("retained PDF receipt changed")
        else:
            payload = common.canonical(result) + b"\n"
            if len(payload) > worker.MAX_RESULT_BYTES:
                raise ValueError("PDF receipt size exceeds its bound")
            with args.output.open("xb") as stream:
                stream.write(payload)
        print(
            common.canonical(
                {
                    "fonts": len(result["result"]["fonts"]),
                    "pdf_sha256": result["result"]["pdf"]["sha256"],
                }
            ).decode()
        )
        return 0
    except (OSError, ValueError) as error:
        print(f"pdf_font_resources: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
