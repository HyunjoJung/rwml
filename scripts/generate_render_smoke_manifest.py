#!/usr/bin/env python3
"""Generate the strict 12-case local render-oracle smoke manifest.

The profile selects existing digest-locked public inputs. It does not generate or
copy document payloads, define a fidelity threshold, or join release validation.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any

try:
    from render_oracle_contract import (
        CORPUS_SCHEMA,
        CorpusDocument,
        CorpusManifest,
        load_corpus_manifest,
    )
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.render_oracle_contract import (
        CORPUS_SCHEMA,
        CorpusDocument,
        CorpusManifest,
        load_corpus_manifest,
    )


ROOT = Path(__file__).resolve().parents[1]
SOURCE_MANIFEST = ROOT / "corpus" / "public" / "RENDER_ORACLE.json"
OUTPUT_MANIFEST = ROOT / "corpus" / "public" / "RENDER_SMOKE_ORACLE.json"
CAMPAIGN = "public-corpus-smoke-v1"

SMOKE_CASE_IDS = (
    "python-docx-test",
    "synthetic-fields",
    "synthetic-floating-wrap-policy",
    "synthetic-floating-z-order-pair",
    "synthetic-kitchen-sink",
    "synthetic-pagination-keep",
    "synthetic-revisions",
    "synthetic-rtl-table",
    "synthetic-style-hidden-tabs-table",
    "synthetic-table-cell-lists",
    "synthetic-two-columns",
    "synthetic-unsupported-objects",
)

EXPECTED_DOCUMENTS = 12
EXPECTED_PAGES = 15
EXPECTED_INPUT_BYTES = 69_027
EXPECTED_PARENT_FEATURES = 37
EXPECTED_COVERED_FEATURES = 35
EXPECTED_OMITTED_FEATURES = ("alternate-content", "top-bottom-wrap")
EXPECTED_WARNING_KINDS = (
    "ChartsPreservedButNotModeled",
    "FloatingShapePlaceholderOnly",
    "OleObjectsPreservedButNotModeled",
    "UnsupportedFieldEvaluation",
    "UnsupportedMetafileImages",
)


def _canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _document_record(document: CorpusDocument) -> dict[str, Any]:
    return {
        "id": document.case_id,
        "path": document.relative_path,
        "format": document.format,
        "bytes": document.input_bytes,
        "sha256": document.sha256,
        "provenance": document.provenance,
        "features": list(document.features),
        "expected": {
            "pages": document.expected_pages,
            "warnings": list(document.expected_warnings),
        },
    }


def _selected_documents() -> tuple[CorpusManifest, tuple[CorpusDocument, ...]]:
    parent = load_corpus_manifest(SOURCE_MANIFEST)
    by_id = {document.case_id: document for document in parent.documents}
    if len(by_id) != len(parent.documents):
        raise ValueError("parent manifest contains duplicate document IDs")
    missing = sorted(set(SMOKE_CASE_IDS) - set(by_id))
    if missing:
        raise ValueError(f"smoke cases are absent from the parent manifest: {missing}")
    selected = tuple(by_id[case_id] for case_id in SMOKE_CASE_IDS)
    if tuple(document.case_id for document in selected) != tuple(
        sorted(SMOKE_CASE_IDS)
    ):
        raise ValueError("smoke case IDs must be canonical and sorted")
    return parent, selected


def profile_summary() -> dict[str, Any]:
    parent, selected = _selected_documents()
    parent_features = set().union(
        *(set(document.features) for document in parent.documents)
    )
    covered_features = set().union(
        *(set(document.features) for document in selected)
    )
    summary = {
        "documents": len(selected),
        "expected_pages": sum(document.expected_pages for document in selected),
        "input_bytes": sum(document.input_bytes for document in selected),
        "parent_features": len(parent_features),
        "covered_features": len(covered_features),
        "omitted_features": sorted(parent_features - covered_features),
        "expected_warning_kinds": sorted(
            set().union(
                *(set(document.expected_warnings) for document in selected)
            )
        ),
    }
    expected = {
        "documents": EXPECTED_DOCUMENTS,
        "expected_pages": EXPECTED_PAGES,
        "input_bytes": EXPECTED_INPUT_BYTES,
        "parent_features": EXPECTED_PARENT_FEATURES,
        "covered_features": EXPECTED_COVERED_FEATURES,
        "omitted_features": list(EXPECTED_OMITTED_FEATURES),
        "expected_warning_kinds": list(EXPECTED_WARNING_KINDS),
    }
    if summary != expected:
        raise ValueError(f"smoke profile coverage drifted: {summary}")
    return summary


def build_manifest() -> dict[str, Any]:
    parent, selected = _selected_documents()
    profile_summary()
    selected_provenance = {document.provenance for document in selected}
    provenance = [
        dict(item) for item in parent.provenance if item["id"] in selected_provenance
    ]
    if {item["id"] for item in provenance} != selected_provenance:
        raise ValueError("smoke profile provenance coverage is incomplete")
    return {
        "schema": CORPUS_SCHEMA,
        "campaign": CAMPAIGN,
        "limits": {
            "max_documents": EXPECTED_DOCUMENTS,
            "max_input_bytes": 64 * 1024,
            "max_total_input_bytes": 128 * 1024,
            "max_pages_per_document": 8,
        },
        "provenance": provenance,
        "documents": [_document_record(document) for document in selected],
    }


def expected_manifest_bytes() -> bytes:
    return _canonical_json(build_manifest())


def _atomic_write(path: Path, payload: bytes) -> None:
    if path.is_symlink():
        raise ValueError("smoke manifest output must not be a symlink")
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


def refresh(path: Path = OUTPUT_MANIFEST) -> None:
    _atomic_write(path, expected_manifest_bytes())
    if path == OUTPUT_MANIFEST:
        load_corpus_manifest(path)


def check(path: Path = OUTPUT_MANIFEST) -> bool:
    try:
        if path.is_symlink():
            raise ValueError("smoke manifest output must not be a symlink")
        actual = path.read_bytes()
        expected = expected_manifest_bytes()
        if actual != expected:
            raise ValueError("smoke manifest is stale")
        if path == OUTPUT_MANIFEST:
            load_corpus_manifest(path)
        return True
    except (OSError, ValueError) as error:
        print(f"generate_render_smoke_manifest: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the strict 12-case render smoke manifest."
    )
    parser.add_argument("--output", type=Path, default=OUTPUT_MANIFEST)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--check", action="store_true")
    action.add_argument("--refresh", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.refresh:
            refresh(args.output)
            print(f"wrote {args.output}")
            return 0
        return 0 if check(args.output) else 1
    except (OSError, ValueError) as error:
        print(f"generate_render_smoke_manifest: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
