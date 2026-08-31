#!/usr/bin/env python3
"""Discover and independently attest every extracted native CFF font in a PDF."""

from __future__ import annotations

import argparse
import base64
import math
from pathlib import Path
import re
import sys
import tempfile
import time
import uuid

import cff_mapping_worker as mapping
import font_subset_attestation as attestation
import font_subset_worker as common
import libreoffice_container as runtime
import pdf_font_resources as resources
import shared_oracle_fonts as shared

WORKER = Path(__file__).with_name("cff_mapping_worker.py")
SCRATCH = runtime.ROOT / "target/render-oracle/cff-discovery"
BATCH_SECONDS = 120


def validate_mapping_result(result: object, request: dict) -> None:
    mapping.validate_request(request)
    expected = {
        **request,
        "schema": "rwml.cff-discovery-worker.v1",
        "fonttools_version": common.WHEEL_VERSION,
        "fonttools_sha256": common.WHEEL_SHA256,
        "pypdf_version": mapping.pdf.WHEEL_VERSION,
        "pypdf_sha256": mapping.pdf.WHEEL_SHA256,
        "python": common.PYTHON_VERSION,
        "limits": {**common.LIMITS, **mapping.MAPPING_LIMITS},
    }
    if (
        not isinstance(result, dict)
        or set(result) != set(expected) | {"glyphs", "stats"}
        or common.canonical({key: result[key] for key in expected})
        != common.canonical(expected)
    ):
        raise ValueError("CFF discovery result identity differs")
    common.validate_cff_map(result["glyphs"])
    stats = result["stats"]
    bounds = {
        "source_glyphs": mapping.MAPPING_LIMITS["candidate_source_glyphs"],
        "outline_commands": mapping.MAPPING_LIMITS["candidate_commands"],
        "search_steps": mapping.MAPPING_LIMITS["candidate_search_steps"],
    }
    if (
        not isinstance(stats, dict)
        or set(stats) != set(bounds)
        or any(
            type(stats[key]) is not int or not 0 <= stats[key] <= maximum
            for key, maximum in bounds.items()
        )
        or stats["source_glyphs"] < len(result["glyphs"])
    ):
        raise ValueError("CFF discovery work statistics differ")


def discover_program(
    program: bytes,
    cmap: bytes,
    source: bytes,
    entry: dict,
    fonttools: Path,
    pypdf: Path,
    *,
    timeout: float = 30,
) -> dict:
    if not math.isfinite(timeout) or not 0 < timeout <= 30:
        raise ValueError("CFF discovery timeout is outside its bound")
    code = runtime.read_regular_file(WORKER, 1024 * 1024)
    helpers = {
        name: runtime.read_regular_file(WORKER.with_name(name), 1024 * 1024)
        for name in mapping.HELPERS
    }
    request = {
        "schema": "rwml.cff-discovery-request.v1",
        "source": {
            key: entry[key]
            for key in ("bytes", "sha256", "postscript_name", "sfnt_revision")
        },
        "program": {"bytes": len(program), "sha256": common.digest(program)},
        "cmap": {"bytes": len(cmap), "sha256": common.digest(cmap)},
        "worker_sha256": common.digest(code),
        "helpers": {name: common.digest(value) for name, value in helpers.items()},
    }
    mapping.validate_request(request)
    if (
        len(source) != entry["bytes"]
        or common.digest(source) != entry["sha256"]
        or not source.startswith(b"OTTO")
        or not program.startswith(b"\x01\x00")
    ):
        raise ValueError("CFF discovery input identity differs")
    wheels = {
        "fonttools.whl": attestation.wheel_payload(fonttools),
        "pypdf.whl": resources.wheel_payload(pypdf),
    }
    lock = runtime.load_runtime_lock()
    image = runtime.inspect_image(lock)
    files = {
        "worker.py": code,
        "source.otf": source,
        "subset.cff": program,
        "unicode.cmap": cmap,
        "request.json": common.canonical(request),
        **helpers,
        **wheels,
    }
    SCRATCH.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="capture-", dir=SCRATCH) as temporary:
        directory = Path(temporary)
        for name, value in files.items():
            with (directory / name).open("xb") as stream:
                stream.write(value)
        name = "rwml-oracle-" + uuid.uuid4().hex
        output = runtime.run_container(
            attestation.worker_command(image, name, directory),
            name,
            timeout=timeout,
            stdout_limit=common.MAX_RESULT_BYTES,
        )
        for name, value in files.items():
            if runtime.read_regular_file(directory / name, len(value)) != value:
                raise ValueError("CFF discovery input changed")
    result = resources.strict_json(output)
    validate_mapping_result(result, request)
    if (
        runtime.read_regular_file(WORKER, 1024 * 1024) != code
        or any(
            runtime.read_regular_file(WORKER.with_name(name), 1024 * 1024) != value
            for name, value in helpers.items()
        )
        or runtime.load_runtime_lock() != lock
        or runtime.inspect_image(lock) != image
        or attestation.wheel_payload(fonttools) != wheels["fonttools.whl"]
        or resources.wheel_payload(pypdf) != wheels["pypdf.whl"]
    ):
        raise ValueError("CFF discovery environment changed")
    return {
        "schema": "rwml.cff-discovery.v1",
        "runtime_lock_sha256": common.digest(common.canonical(lock)),
        "image_manifest_sha256": lock["image"]["manifest_sha256"],
        "fonttools_lock_sha256": common.digest(
            common.canonical(attestation.tool_lock())
        ),
        "pypdf_lock_sha256": common.digest(common.canonical(resources.tool_lock())),
        "result": result,
    }


def attest_pdf(
    payload: bytes, source: bytes, entry: dict, fonttools: Path, pypdf: Path
) -> dict:
    deadline = time.monotonic() + BATCH_SECONDS

    def remaining():
        value = deadline - time.monotonic()
        if value <= 0:
            raise ValueError("native CFF PDF batch timed out")
        return min(30, value)

    extraction = resources.extract_pdf(payload, pypdf, timeout=remaining())
    data = extraction["result"]
    blobs = {tuple(blob["ref"]): blob for blob in data["blobs"]}
    proofs, unverified = [], []
    result = {
        "schema": "rwml.native-cff-pdf-attestation.v1",
        "scope": "native-cff-glyph-outlines-only",
        "batch_seconds": BATCH_SECONDS,
        "extraction": extraction,
        "cff_resources": proofs,
        "unverified_resources": unverified,
    }
    for font in data["fonts"]:
        program_blob = blobs[tuple(font["program"])]
        if program_blob["kind"] != "cid-cff":
            unverified.append({"font_ref": font["ref"], "kind": program_blob["kind"]})
            continue
        if (
            font["to_unicode"] is None
            or re.sub(r"^[A-Z]{6}\+", "", font["descriptor_font"])
            != entry["postscript_name"]
        ):
            raise ValueError("native CFF resource source or ToUnicode is unavailable")
        program = base64.b64decode(program_blob["base64"], validate=True)
        cmap = base64.b64decode(
            blobs[tuple(font["to_unicode"])]["base64"], validate=True
        )
        witness = discover_program(
            program, cmap, source, entry, fonttools, pypdf, timeout=remaining()
        )
        proof = attestation.attest_program(
            program,
            source,
            entry,
            fonttools,
            glyph_map=witness["result"]["glyphs"],
            timeout=remaining(),
        )
        for key in ("runtime_lock_sha256", "image_manifest_sha256"):
            if not extraction[key] == witness[key] == proof[key]:
                raise ValueError("native CFF pipeline runtime differs")
        if (
            witness["pypdf_lock_sha256"] != extraction["tool_lock_sha256"]
            or witness["fonttools_lock_sha256"] != proof["tool_lock_sha256"]
            or witness["result"]["helpers"]["pdf_font_worker.py"]
            != data["worker_sha256"]
            or not witness["result"]["helpers"]["font_subset_worker.py"]
            == data["helper_sha256"]
            == proof["result"]["worker_sha256"]
        ):
            raise ValueError("native CFF pipeline tool identities differ")
        proofs.append({"font_ref": font["ref"], "discovery": witness, "proof": proof})
        if len(common.canonical(result)) + 1 > mapping.pdf.MAX_RESULT_BYTES:
            raise ValueError("native CFF PDF receipt exceeds its bound")
    if not proofs:
        raise ValueError("PDF contains no native CFF resources to attest")
    remaining()
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--font-pack", type=Path, required=True)
    parser.add_argument("--fonttools-wheel", type=Path, required=True)
    parser.add_argument("--pypdf-wheel", type=Path, required=True)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--output", type=Path)
    mode.add_argument("--verify", type=Path)
    args = parser.parse_args()
    try:
        if args.output and (args.output.exists() or args.output.is_symlink()):
            raise ValueError("output must be fresh")
        retained = (
            runtime.read_regular_file(args.verify, mapping.pdf.MAX_RESULT_BYTES)
            if args.verify
            else None
        )
        if retained is not None:
            resources.strict_json(retained)
        lock = shared.load_lock()
        shared.verify_pack(args.font_pack, lock)
        entry = next(
            item for item in lock.fonts if item["name"] == "NotoSansCJKkr-Regular.otf"
        )
        source = runtime.read_regular_file(
            args.font_pack / "fonts" / entry["name"], entry["bytes"]
        )
        payload = runtime.read_regular_file(args.pdf, mapping.pdf.MAX_PDF_BYTES)
        result = attest_pdf(
            payload, source, entry, args.fonttools_wheel, args.pypdf_wheel
        )
        result["shared_font_lock_sha256"] = lock.sha256
        result["shared_base_lock_sha256"] = lock.base_sha256
        shared.verify_pack(args.font_pack, lock)
        if runtime.read_regular_file(args.pdf, mapping.pdf.MAX_PDF_BYTES) != payload:
            raise ValueError("native CFF PDF input changed")
        if args.verify:
            resources.verify_receipt(retained, result)
            if (
                runtime.read_regular_file(args.verify, mapping.pdf.MAX_RESULT_BYTES)
                != retained
            ):
                raise ValueError("native CFF retained receipt changed")
        else:
            serialized = common.canonical(result) + b"\n"
            if len(serialized) > mapping.pdf.MAX_RESULT_BYTES:
                raise ValueError("native CFF PDF receipt exceeds its bound")
            with args.output.open("xb") as stream:
                stream.write(serialized)
        print(
            common.canonical(
                {
                    "cff_fonts": len(result["cff_resources"]),
                    "other_fonts_unverified": len(result["unverified_resources"]),
                    "glyphs": sum(
                        item["proof"]["result"]["proof"]["glyph_count"]
                        for item in result["cff_resources"]
                    ),
                }
            ).decode()
        )
        return 0
    except (OSError, ValueError, StopIteration) as error:
        print(f"native_cff_attestation: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
