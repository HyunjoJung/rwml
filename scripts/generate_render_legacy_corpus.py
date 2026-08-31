#!/usr/bin/env python3
"""Materialize the reviewed legacy-DOC render corpus batch."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile

try:
    from bench_vs_mature import legacy_benchmark_inputs
    from render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.bench_vs_mature import legacy_benchmark_inputs
    from scripts.render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = Path(__file__).resolve()
BENCHMARK_TOOL = ROOT / "scripts" / "bench_vs_mature.py"
BENCHMARK_ROOT = ROOT / "corpus" / "public" / "benchmark"
BENCHMARK_MANIFEST = BENCHMARK_ROOT / "LEGACY_MANIFEST.tsv"
BENCHMARK_README = BENCHMARK_ROOT / "README.md"
ATTRIBUTION = ROOT / "corpus" / "public" / "ATTRIBUTION.md"
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-legacy-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-legacy-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-legacy-v1"
PROVENANCE_ID = "rwml-render-full-legacy"
PROVENANCE_PATH = "provenance/rwml-render-full-legacy.md"
CASE_COUNT = 3
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 192 * 1024
MAX_REFERENCE_BYTES = 16 * 1024

PROVENANCE_TEXT = """# Public full-render legacy-DOC batch provenance

The three `full-legacy-*` inputs are exact copies of the generated Word 97-2003
`.doc` fixtures in `corpus/public/benchmark/sample/`. LibreOffice 26.2.3.2
exported them with its `MS Word 97` filter on 2026-07-15 from the corresponding
repository-owned synthetic `.docx` inputs recorded in
`corpus/public/ATTRIBUTION.md`.

The checked-in batch lock binds each `.doc` payload, its synthetic `.docx`
source, and the exact text references produced by Apache POI 5.2.3 and
LibreOffice 26.2.3.2. The inputs and generated provenance record are licensed
under the repository's MIT license. This fixed reviewed subset establishes
bounded native legacy parsing and PDF-render execution for three known inputs.
It does not establish independent Microsoft Word provenance, broad real-world
producer diversity, layout equivalence with either reference extractor,
Word-exact pagination, completion of the planned full corpus, or a release-gate
change.
"""

CASE_FEATURES = {
    "floating_text_bearing": (
        "apache-poi-text-reference",
        "legacy-doc",
        "libreoffice-text-reference",
        "source-floating-text-bearing",
    ),
    "floating_wrap_policy": (
        "apache-poi-text-reference",
        "legacy-doc",
        "libreoffice-text-reference",
        "source-floating-wrap-policy",
    ),
    "nested_tables": (
        "apache-poi-text-reference",
        "legacy-doc",
        "libreoffice-text-reference",
        "source-nested-tables",
    ),
}


@dataclass(frozen=True)
class CaseSpec:
    name: str
    source_doc: Path
    source_docx: Path
    poi_golden: Path
    libreoffice_golden: Path
    features: tuple[str, ...]

    @property
    def case_id(self) -> str:
        return f"full-legacy-{self.name.replace('_', '-')}"

    @property
    def relative_path(self) -> str:
        return f"documents/{self.case_id}.doc"


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _read_source(path: Path, maximum: int) -> bytes:
    if path.is_symlink():
        raise ValueError(f"legacy corpus source must not be a symlink: {path.name}")
    try:
        resolved_root = ROOT.resolve(strict=True)
        resolved = path.resolve(strict=True)
        resolved.relative_to(resolved_root)
    except (FileNotFoundError, ValueError) as error:
        raise ValueError(f"legacy corpus source is outside the repository: {path}") from error
    if not resolved.is_file():
        raise ValueError(f"legacy corpus source is not a regular file: {path}")
    payload = resolved.read_bytes()
    if not payload or len(payload) > maximum:
        raise ValueError(f"legacy corpus source size is invalid: {path}")
    return payload


def _relative(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def _identity(path: Path, maximum: int) -> dict[str, object]:
    payload = _read_source(path, maximum)
    return {
        "bytes": len(payload),
        "path": _relative(path),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }


def case_specs() -> tuple[CaseSpec, ...]:
    benchmark = {item.name: item for item in legacy_benchmark_inputs(BENCHMARK_ROOT)}
    if set(benchmark) != set(CASE_FEATURES):
        raise ValueError("legacy render batch does not match the benchmark inventory")
    specs = tuple(
        CaseSpec(
            name=name,
            source_doc=benchmark[name].document,
            source_docx=ROOT / "corpus" / "public" / "synthetic" / f"{name}.docx",
            poi_golden=benchmark[name].poi_golden,
            libreoffice_golden=benchmark[name].libreoffice_golden,
            features=tuple(sorted(CASE_FEATURES[name])),
        )
        for name in sorted(CASE_FEATURES)
    )
    if len(specs) != CASE_COUNT or len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("legacy render batch identities are incomplete")
    return specs


def _validate_case(spec: CaseSpec) -> bytes:
    payload = _read_source(spec.source_doc, MAX_CASE_BYTES)
    if not payload.startswith(bytes.fromhex("d0cf11e0a1b11ae1")):
        raise ValueError(f"legacy render input is not an OLE2 document: {spec.case_id}")
    source_docx = _read_source(spec.source_docx, MAX_CASE_BYTES)
    if not source_docx.startswith(b"PK"):
        raise ValueError(f"legacy render source is not a DOCX package: {spec.case_id}")
    _read_source(spec.poi_golden, MAX_REFERENCE_BYTES).decode("utf-8-sig")
    _read_source(spec.libreoffice_golden, MAX_REFERENCE_BYTES).decode("utf-8-sig")
    return payload


def _reference_record(path: Path, extractor: str, version: str) -> dict[str, object]:
    return {
        **_identity(path, MAX_REFERENCE_BYTES),
        "extractor": extractor,
        "version": version,
    }


def _closure_paths(specs: tuple[CaseSpec, ...]) -> tuple[Path, ...]:
    paths = {
        SCRIPT_PATH,
        BENCHMARK_TOOL,
        BENCHMARK_MANIFEST,
        BENCHMARK_README,
        ATTRIBUTION,
    }
    for spec in specs:
        paths.update(
            (
                spec.source_doc,
                spec.source_docx,
                spec.poi_golden,
                spec.libreoffice_golden,
            )
        )
    return tuple(sorted(paths))


def _generator_closure_sha256(specs: tuple[CaseSpec, ...]) -> str:
    digest = hashlib.sha256()
    for path in _closure_paths(specs):
        relative = _relative(path).encode("utf-8")
        payload = _read_source(path, MAX_CASE_BYTES)
        digest.update(len(relative).to_bytes(8, "little"))
        digest.update(relative)
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


def _provenance_record() -> dict[str, object]:
    payload = PROVENANCE_TEXT.encode("utf-8")
    return {
        "bytes": len(payload),
        "id": PROVENANCE_ID,
        "kind": "converted",
        "license": "MIT",
        "reference": PROVENANCE_PATH,
        "sha256": hashlib.sha256(payload).hexdigest(),
    }


def _coverage() -> dict[str, object]:
    return {
        "case_count": CASE_COUNT,
        "cohort": "bounded-legacy-doc",
        "conversion": {
            "date": "2026-07-15",
            "filter": "MS Word 97",
            "producer": "LibreOffice",
            "version": "26.2.3.2",
        },
        "design": "fixed-reviewed-subset",
        "reference_extractors": ["Apache POI 5.2.3", "LibreOffice 26.2.3.2"],
        "scope": [
            "floating-source-text",
            "nested-table-source",
            "native-legacy-render-execution",
        ],
    }


def build_lock() -> dict[str, object]:
    specs = case_specs()
    documents = []
    payload_hashes = set()
    total_bytes = 0
    for spec in specs:
        payload = _validate_case(spec)
        payload_sha256 = hashlib.sha256(payload).hexdigest()
        if payload_sha256 in payload_hashes:
            raise ValueError(f"duplicate legacy render payload: {spec.case_id}")
        payload_hashes.add(payload_sha256)
        total_bytes += len(payload)
        documents.append(
            {
                "bytes": len(payload),
                "expected": {"pages": 1, "warnings": []},
                "features": list(spec.features),
                "format": "doc",
                "id": spec.case_id,
                "path": spec.relative_path,
                "provenance": PROVENANCE_ID,
                "references": {
                    "apache-poi": _reference_record(
                        spec.poi_golden, "Apache POI", "5.2.3"
                    ),
                    "libreoffice": _reference_record(
                        spec.libreoffice_golden, "LibreOffice", "26.2.3.2"
                    ),
                },
                "sha256": payload_sha256,
                "source": "converted",
                "source_docx": _identity(spec.source_docx, MAX_CASE_BYTES),
                "source_path": _relative(spec.source_doc),
            }
        )
    if total_bytes > MAX_TOTAL_BYTES:
        raise ValueError("legacy render batch total byte limit exceeded")
    return {
        "campaign": CAMPAIGN,
        "coverage": _coverage(),
        "documents": documents,
        "generator_closure_sha256": _generator_closure_sha256(specs),
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
    payloads = {
        spec.relative_path: _validate_case(spec) for spec in case_specs()
    }
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
            materialize(Path(temporary) / "legacy", lock)
        return True
    except (OSError, UnicodeDecodeError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_legacy_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Materialize or verify the three-case legacy-DOC render batch."
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
    except (OSError, UnicodeDecodeError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_legacy_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
