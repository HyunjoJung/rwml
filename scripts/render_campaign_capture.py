#!/usr/bin/env python3
"""Retain shared-font native/LibreOffice captures with independent font checks.

This diagnostic capture bundle is not release evidence or a Word fidelity proof.
"""

from __future__ import annotations

import argparse
import base64
from pathlib import Path
import sys
import tempfile
import time

import font_subset_attestation as attestation
import libreoffice_container as runtime
import libreoffice_table_capture as table_capture
import native_cff_attestation as native
import pdf_font_resources as resources
import render_validate as render
import shared_oracle_fonts as shared
from libreoffice_oracle_fonts import normalized_postscript_name, sfnt_revision
from render_oracle_contract import (
    _assert_path_neutral,
    _load_json,
    load_corpus_manifest,
)

ROOT = runtime.ROOT
SCHEMA = "rwml.render-campaign-capture.v1"
FONT_SCHEMA = "rwml.campaign-font-checks.v1"
MAX_BUNDLE_BYTES = 16 * 1024 * 1024
MAX_RENDERER_BYTES = 256 * 1024 * 1024
MAX_CAMPAIGN_BYTES = 2 * 1024 * 1024 * 1024
MAX_CAMPAIGN_SECONDS = 4 * 60 * 60
canonical = runtime.canonical_json
digest = runtime.sha256
verify_files = table_capture.verify_directory


def require_equal(expected: object, actual: object, label: str) -> None:
    if isinstance(expected, bytes):
        if type(actual) is not bytes or expected != actual:
            raise ValueError(f"{label} differs")
    else:
        table_capture.require_equal(expected, actual, label)


def write_new(path: Path, payload: bytes) -> None:
    with path.open("xb") as stream:
        stream.write(payload)


def identity(payload: bytes) -> dict:
    return {"bytes": len(payload), "sha256": digest(payload)}


def native_command(
    binary: Path, source: Path, pdf: Path, report: Path, fonts: list[Path]
) -> list[str]:
    if not 1 <= len(fonts) <= 128:
        raise ValueError("native capture requires ordered explicit fonts")
    command = [str(binary), str(source), str(pdf), "--report-json", str(report)]
    for font in fonts:
        command.extend(["--font", str(font)])
    return command


def font_files(entries: list | tuple, payloads: dict[str, bytes]) -> dict[str, bytes]:
    if set(payloads) != {entry["name"] for entry in entries}:
        raise ValueError("font input coverage differs")
    names = sorted(payloads)
    return {
        **payloads,
        "SHA256SUMS": "".join(
            f"{digest(payloads[name])}  {name}\n" for name in names
        ).encode(),
        "expected-paths.txt": "".join(
            f"/oracle/fonts/{name}\n" for name in names
        ).encode(),
    }


def validate_capture(entries: dict[str, bytes], font_paths: bytes) -> None:
    if set(entries) != runtime.CAPTURE_MEMBERS:
        raise ValueError("capture file set differs")
    if entries["version.txt"].strip() != runtime.VERSION_LINE.encode():
        raise ValueError("captured runtime version differs")
    if entries["fonts.txt"] != font_paths:
        raise ValueError("captured font closure differs")
    pdf = entries["output.pdf"]
    if not pdf.startswith(b"%PDF-") or len(pdf) > resources.worker.MAX_PDF_BYTES:
        raise ValueError("captured PDF exceeds the parser contract")
    if entries["sha256.txt"] != (digest(pdf) + "  output.pdf\n").encode():
        raise ValueError("captured PDF digest differs")


def source_payload(document) -> bytes:
    payload = runtime.read_regular_file(document.path, document.input_bytes)
    if identity(payload) != {"bytes": document.input_bytes, "sha256": document.sha256}:
        raise ValueError("campaign source document changed")
    return payload


def check_fonts(
    payload: bytes,
    entries: list | tuple,
    sources: dict[str, bytes],
    fonttools: Path,
    pypdf: Path,
) -> dict:
    deadline = time.monotonic() + 180
    extraction = resources.extract_pdf(payload, pypdf)
    data = extraction["result"]
    blobs = {tuple(blob["ref"]): blob for blob in data["blobs"]}
    locked = {entry["postscript_name"]: entry for entry in entries}
    cff_fonts = [
        font
        for font in data["fonts"]
        if blobs[tuple(font["program"])]["kind"] == "cid-cff"
    ]
    cff = None
    if cff_fonts:
        cff_entries = [entry for entry in entries if entry["format"] == "opentype-cff"]
        if len(cff_entries) != 1:
            raise ValueError("native CFF source selection is ambiguous")
        entry = cff_entries[0]
        cff = native.attest_pdf(
            payload, sources[entry["name"]], entry, fonttools, pypdf
        )
        require_equal(extraction, cff["extraction"], "CFF extraction")
        require_equal(
            [font["ref"] for font in cff_fonts],
            [proof["font_ref"] for proof in cff["cff_resources"]],
            "CFF coverage",
        )
    checks = []
    for font in data["fonts"]:
        if time.monotonic() > deadline:
            raise ValueError("campaign PDF font checks timed out")
        name = normalized_postscript_name(font["descriptor_font"])
        entry = locked.get(name)
        if entry is None:
            raise ValueError("PDF font is outside the shared source lock")
        blob = blobs[tuple(font["program"])]
        program = base64.b64decode(blob["base64"], validate=True)
        row = {"font_ref": font["ref"], "source": entry["name"], "kind": blob["kind"]}
        if blob["kind"] == "truetype":
            if (
                entry["format"] not in {"truetype", "truetype-variable"}
                or sfnt_revision(program) != entry["sfnt_revision"]
            ):
                raise ValueError("PDF TrueType metadata differs from shared source")
            row["check"] = "postscript-and-revision-only"
        elif blob["kind"] == "type1-pfa" and entry["format"] == "opentype-cff":
            proof = attestation.attest_program(
                program, sources[entry["name"]], entry, fonttools
            )
            for key in ("runtime_lock_sha256", "image_manifest_sha256"):
                require_equal(extraction[key], proof[key], "Type 1 runtime")
            row.update(check="exact-glyph-outlines", proof=proof)
        elif blob["kind"] == "cid-cff" and entry["format"] == "opentype-cff":
            row["check"] = "exact-glyph-outlines"
        else:
            raise ValueError("PDF font representation has no applicable check")
        checks.append(row)
    return {
        "schema": FONT_SCHEMA,
        "scope": "declared-font-resources",
        "extraction": extraction,
        "native_cff": cff,
        "resources": checks,
    }


def harness_identity() -> dict:
    result = table_capture.harness_identity()
    for name in (
        "render_campaign_capture.py",
        "shared_oracle_fonts.py",
        "pdf_font_resources.py",
        "pdf_font_worker.py",
        "font_subset_worker.py",
        "font_subset_attestation.py",
        "native_cff_attestation.py",
        "cff_mapping_worker.py",
    ):
        result[name] = digest(
            runtime.read_regular_file(ROOT / "scripts" / name, 1024 * 1024)
        )
    return dict(sorted(result.items()))


def prepare_environment(
    pack: Path, fonttools: Path, pypdf: Path
) -> tuple[dict, dict, object, dict]:
    lock = shared.load_lock()
    receipt = shared.verify_pack(pack, lock)
    sources = shared._read_inputs(pack / "fonts", lock.fonts, shared.MAX_FONT_BYTES)
    runtime_lock = runtime.load_runtime_lock()
    image = runtime.inspect_image(runtime_lock)
    attestation.wheel_payload(fonttools)
    resources.wheel_payload(pypdf)
    material = {
        "runtime": runtime_lock,
        "executor": table_capture.execution_identity(),
        "font_pack": receipt,
        "fonttools": attestation.tool_lock(),
        "pypdf": resources.tool_lock(),
        "harness": harness_identity(),
        "analysis_tools": table_capture.analysis_tools(),
    }
    numpy = render.integer_metric_numpy()
    if numpy is not None:
        material["analysis_tools"]["numpy"] = str(numpy.__version__)
    _assert_path_neutral(material)
    return material, sources, lock, {"image": image}


def build_renderer(output: Path) -> dict:
    env = render.rust_tool_environment()
    command = [
        "rustup",
        "run",
        "1.92.0",
        "cargo",
        "build",
        "--offline",
        "--locked",
        "--features",
        "render",
        "--example",
        "to_pdf",
        "--message-format=json",
        "--target-dir",
        str(ROOT / "target/codex-1.92"),
    ]
    log = runtime.run_bounded(
        command, cwd=ROOT, env=env, timeout=900, stdout_limit=4 * 1024 * 1024
    )
    executables = []
    for line in log.splitlines():
        row = resources.strict_json(line)
        if (
            row.get("reason") == "compiler-artifact"
            and row.get("target", {}).get("name") == "to_pdf"
            and row.get("executable")
        ):
            executables.append(Path(row["executable"]))
    if len(executables) != 1:
        raise ValueError("native renderer build artifact is ambiguous")
    payload = runtime.read_regular_file(executables[0], MAX_RENDERER_BYTES)
    write_new(output, payload)
    output.chmod(0o755)
    rustc = (
        runtime.run_bounded(["rustup", "run", "1.92.0", "rustc", "--version"], env=env)
        .decode()
        .strip()
    )
    return {
        **identity(payload),
        "rustc": rustc,
        "features": ["render"],
        "profile": "dev",
        "cargo_lock_sha256": digest((ROOT / "Cargo.lock").read_bytes()),
    }


def read_artifacts(directory: Path, names: set[str], maximum: int) -> dict[str, bytes]:
    if (
        directory.is_symlink()
        or not directory.is_dir()
        or {p.name for p in directory.iterdir()} != names
    ):
        raise ValueError("retained artifact set differs")
    return {
        name: runtime.read_regular_file(
            directory / name,
            maximum if name == "output.pdf" else 65536,
            allow_empty=True,
        )
        for name in sorted(names)
    }


def pdf_record(
    pdf: Path,
    check_path: Path,
    sources: dict,
    lock,
    fonttools: Path,
    pypdf: Path,
    *,
    verify: bool,
) -> dict:
    payload = runtime.read_regular_file(pdf, resources.worker.MAX_PDF_BYTES)
    checks = check_fonts(payload, lock.fonts, sources, fonttools, pypdf)
    serialized = canonical(checks) + b"\n"
    if len(serialized) > resources.worker.MAX_RESULT_BYTES:
        raise ValueError("campaign font receipt exceeds its bound")
    if verify:
        retained = runtime.read_regular_file(
            check_path, resources.worker.MAX_RESULT_BYTES
        )
        resources.verify_receipt(retained, checks)
        require_equal(
            retained,
            runtime.read_regular_file(check_path, len(retained)),
            "retained receipt",
        )
    else:
        write_new(check_path, serialized)
    require_equal(payload, runtime.read_regular_file(pdf, len(payload)), "retained PDF")
    return {
        "pdf": identity(payload),
        "font_checks": identity(serialized),
        "resources": [
            {key: row[key] for key in ("font_ref", "source", "kind", "check")}
            for row in checks["resources"]
        ],
    }


def inspect_case(
    directory: Path,
    document,
    sources: dict,
    lock,
    fonttools: Path,
    pypdf: Path,
    font_paths: bytes,
    *,
    verify: bool,
) -> dict:
    expected = {
        "input.docx",
        "SHA256SUMS",
        "native.pdf",
        "native-report.json",
        "reference",
    }
    if verify:
        expected |= {"native-fonts.json", "reference-fonts.json"}
    if directory.is_symlink() or {p.name for p in directory.iterdir()} != expected:
        raise ValueError("case artifact coverage differs")
    source = source_payload(document)
    require_equal(
        source,
        runtime.read_regular_file(directory / "input.docx", document.input_bytes),
        "retained input",
    )
    require_equal(
        (digest(source) + "  input.docx\n").encode(),
        runtime.read_regular_file(directory / "SHA256SUMS", 1024),
        "input checksum",
    )
    reference = read_artifacts(
        directory / "reference", runtime.CAPTURE_MEMBERS, runtime.MAX_PDF_BYTES
    )
    validate_capture(reference, font_paths)
    report, report_payload = _load_json(directory / "native-report.json", 1024 * 1024)
    if render.warning_kinds(report) is None:
        raise ValueError("native render warnings are invalid")
    result = {
        "case_id": document.case_id,
        "input": identity(source),
        "native_report": identity(report_payload),
        "reference_auxiliary": {
            name: identity(data)
            for name, data in reference.items()
            if name != "output.pdf"
        },
    }
    for name, path in (
        ("native", directory / "native.pdf"),
        ("reference", directory / "reference/output.pdf"),
    ):
        result[name] = pdf_record(
            path,
            directory / f"{name}-fonts.json",
            sources,
            lock,
            fonttools,
            pypdf,
            verify=verify,
        )
    return result


def run(
    manifest: Path,
    output: Path,
    pack: Path,
    fonttools: Path,
    pypdf: Path,
    *,
    verify: bool = False,
) -> dict:
    if not verify and (output.exists() or output.is_symlink()):
        raise ValueError("capture output must be fresh")
    revision = table_capture.source_revision()
    corpus = load_corpus_manifest(manifest)
    if any(document.format != "docx" for document in corpus.documents):
        raise ValueError("locked campaign capture currently requires DOCX inputs")
    if output.resolve().is_relative_to(
        pack.resolve()
    ) or output.resolve().is_relative_to(corpus.path.parent.resolve()):
        raise ValueError("capture output overlaps its inputs")
    material, sources, lock, execution = prepare_environment(pack, fonttools, pypdf)
    staged_fonts = font_files(lock.fonts, sources)
    deadline = time.monotonic() + MAX_CAMPAIGN_SECONDS
    if verify:
        retained, retained_bytes = _load_json(output / "CAPTURE.json", MAX_BUNDLE_BYTES)
        require_equal(
            revision, retained.get("source_revision"), "capture source revision"
        )
        require_equal(material, retained.get("environment"), "capture environment")
        require_equal(corpus.identity(), retained.get("campaign"), "capture campaign")
        renderer = retained.get("renderer")
        if not isinstance(renderer, dict) or set(renderer) != {
            "bytes",
            "sha256",
            "rustc",
            "features",
            "profile",
            "cargo_lock_sha256",
        }:
            raise ValueError("renderer identity is malformed")
        require_equal(
            identity(
                runtime.read_regular_file(output / "renderer", MAX_RENDERER_BYTES)
            ),
            {key: renderer[key] for key in ("bytes", "sha256")},
            "native executable",
        )
        require_equal(
            digest((ROOT / "Cargo.lock").read_bytes()),
            renderer["cargo_lock_sha256"],
            "Cargo lock",
        )
        require_equal(["render"], renderer["features"], "renderer features")
        require_equal("dev", renderer["profile"], "renderer profile")
        scratch = ROOT / "target/render-oracle/capture-build-verification"
        scratch.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=scratch) as temporary:
            require_equal(
                renderer,
                build_renderer(Path(temporary) / "renderer"),
                "independently rebuilt renderer",
            )
        if {p.name for p in output.iterdir()} != {
            "CAPTURE.json",
            "renderer",
            "fonts",
            "cases",
        } or output.is_symlink():
            raise ValueError("capture root coverage differs")
    else:
        output.mkdir(parents=True, exist_ok=False)
        renderer = build_renderer(output / "renderer")
        (output / "fonts").mkdir()
        for name, payload in staged_fonts.items():
            write_new(output / "fonts" / name, payload)
        (output / "cases").mkdir()
    verify_files(output / "fonts", staged_fonts)
    rows, total_bytes = (
        [],
        renderer["bytes"] + sum(len(payload) for payload in staged_fonts.values()),
    )
    if verify and (output / "cases").is_symlink():
        raise ValueError("case directory is a symlink")
    if verify and {p.name for p in (output / "cases").iterdir()} != {
        doc.case_id for doc in corpus.documents
    }:
        raise ValueError("retained campaign coverage differs")
    for document in corpus.documents:
        if time.monotonic() > deadline:
            raise ValueError("render campaign capture timed out")
        directory = output / "cases" / document.case_id
        if not verify:
            directory.mkdir()
            payload = source_payload(document)
            write_new(directory / "input.docx", payload)
            write_new(
                directory / "SHA256SUMS", (digest(payload) + "  input.docx\n").encode()
            )
            captured = runtime.capture_document(
                execution["image"], directory, output / "fonts"
            )
            validate_capture(captured, staged_fonts["expected-paths.txt"])
            (directory / "reference").mkdir()
            for name, content in captured.items():
                write_new(directory / "reference" / name, content)
            runtime.run_bounded(
                native_command(
                    output / "renderer",
                    directory / "input.docx",
                    directory / "native.pdf",
                    directory / "native-report.json",
                    [output / "fonts" / entry["name"] for entry in lock.fonts],
                ),
                timeout=120,
                cwd=ROOT,
            )
        row = inspect_case(
            directory,
            document,
            sources,
            lock,
            fonttools,
            pypdf,
            staged_fonts["expected-paths.txt"],
            verify=verify,
        )
        total_bytes += sum(
            path.stat().st_size for path in directory.rglob("*") if path.is_file()
        )
        if total_bytes > MAX_CAMPAIGN_BYTES:
            raise ValueError("retained campaign exceeds its aggregate byte bound")
        rows.append(row)
        print(
            f"{len(rows)}/{len(corpus.documents)} {document.case_id}",
            file=sys.stderr,
            flush=True,
        )
    require_equal(
        revision, table_capture.source_revision(revision), "source after capture"
    )
    if load_corpus_manifest(manifest) != corpus:
        raise ValueError("corpus changed after capture")
    final_material, _, _, final_execution = prepare_environment(pack, fonttools, pypdf)
    require_equal(material, final_material, "environment after capture")
    require_equal(execution, final_execution, "runtime after capture")
    verify_files(output / "fonts", staged_fonts)
    require_equal(
        {key: renderer[key] for key in ("bytes", "sha256")},
        identity(runtime.read_regular_file(output / "renderer", MAX_RENDERER_BYTES)),
        "native executable after capture",
    )
    if time.monotonic() > deadline:
        raise ValueError("render campaign capture timed out")
    result = {
        "schema": SCHEMA,
        "scope": "diagnostic-capture-not-release-evidence",
        "source_revision": revision,
        "campaign": corpus.identity(),
        "environment": material,
        "renderer": renderer,
        "limits": {"seconds": MAX_CAMPAIGN_SECONDS, "bytes": MAX_CAMPAIGN_BYTES},
        "rows": rows,
    }
    _assert_path_neutral(result)
    serialized = canonical(result) + b"\n"
    if len(serialized) > MAX_BUNDLE_BYTES:
        raise ValueError("capture receipt exceeds its bound")
    if verify:
        require_equal(retained, result, "independently recomputed capture")
        if (
            runtime.read_regular_file(output / "CAPTURE.json", MAX_BUNDLE_BYTES)
            != retained_bytes
        ):
            raise ValueError("retained capture receipt changed")
    else:
        write_new(output / "CAPTURE.json", serialized)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("capture", "verify"))
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--font-pack", type=Path, required=True)
    parser.add_argument("--fonttools-wheel", type=Path, required=True)
    parser.add_argument("--pypdf-wheel", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = run(
            args.manifest.absolute(),
            args.output.absolute(),
            args.font_pack.absolute(),
            args.fonttools_wheel.absolute(),
            args.pypdf_wheel.absolute(),
            verify=args.mode == "verify",
        )
        print(
            canonical(
                {"documents": len(result["rows"]), "scope": result["scope"]}
            ).decode()
        )
        return 0
    except (OSError, ValueError) as error:
        print(f"render_campaign_capture: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
