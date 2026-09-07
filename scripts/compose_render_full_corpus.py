#!/usr/bin/env python3
"""Compose the exact 800-input render campaign from checked-in source locks."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import importlib
import json
from pathlib import Path
import sys
import tempfile

try:
    import render_corpus_batch as batch
except ModuleNotFoundError:
    from scripts import render_corpus_batch as batch


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LOCK = ROOT / "corpus/public/oracle/render-full-v1.json"
DEFAULT_OUTPUT = ROOT / "target/render-oracle/render-full-v1"
CASE_COUNT = 800
CAMPAIGN = "public-render-full-v1"
SOURCE_MODULES = (
    "generate_render_full_corpus",
    "generate_render_paragraph_corpus",
    "generate_render_list_rtl_corpus",
    "generate_render_table_corpus",
    "generate_render_section_corpus",
    "generate_render_note_field_corpus",
    "generate_render_metafile_corpus",
    "generate_render_floating_corpus",
    "generate_render_legacy_corpus",
    "generate_render_pilot",
    "generate_unequal_table_oracle",
    "generate_render_interaction_corpus",
)


@dataclass(frozen=True)
class SourceBatch:
    lock_path: Path
    lock_bytes: bytes
    lock: dict[str, object]
    payloads: dict[str, bytes]


def source_batches() -> tuple[SourceBatch, ...]:
    sources = []
    for name in SOURCE_MODULES:
        module = importlib.import_module(
            f"{__package__}.{name}" if __package__ else name
        )
        if name == "generate_unequal_table_oracle":
            module.verify_lock()
            artifacts = module.campaign_artifacts()
            lock = json.loads(artifacts.pop("RENDER_ORACLE.json"))
            provenance = lock["provenance"][0]
            payload = artifacts.pop(provenance["reference"])
            provenance.update(
                {
                    "reference": "provenance/synthetic-unequal-table.md",
                    "bytes": len(payload),
                    "sha256": hashlib.sha256(payload).hexdigest(),
                }
            )
            artifacts[provenance["reference"]] = payload
        elif name == "generate_render_interaction_corpus":
            current, artifacts = module._build_batch()
            lock = batch.load_lock(module.DEFAULT_LOCK, current)
        else:
            lock = module.load_lock()
            artifacts = module._payloads()
        sources.append(
            SourceBatch(
                module.DEFAULT_LOCK, module.DEFAULT_LOCK.read_bytes(), lock, artifacts
            )
        )
    return tuple(sources)


def compose(
    sources: tuple[SourceBatch, ...],
) -> tuple[dict[str, object], dict[str, bytes]]:
    documents = []
    provenance = {}
    payloads = {}
    source_records = []
    ids, hashes, source_names = set(), set(), set()
    for source in sources:
        name = source.lock["campaign"]
        if name in source_names:
            raise ValueError(f"duplicate source batch: {name}")
        source_names.add(name)
        for document in source.lock["documents"]:
            if document["id"] in ids:
                raise ValueError(f"duplicate document id: {document['id']}")
            if document["sha256"] in hashes:
                raise ValueError(f"duplicate document payload: {document['id']}")
            ids.add(document["id"])
            hashes.add(document["sha256"])
            documents.append({**document, "source_batch": name})
        for item in source.lock["provenance"]:
            if item["id"] in provenance and provenance[item["id"]] != item:
                raise ValueError(f"conflicting provenance id: {item['id']}")
            provenance[item["id"]] = item
        batch.validate_payloads(source.lock, source.payloads)
        for path, payload in source.payloads.items():
            if path in payloads:
                raise ValueError(f"duplicate payload path: {path}")
            payloads[path] = payload
        source_records.append(
            {
                "campaign": name,
                "path": source.lock_path.relative_to(ROOT).as_posix(),
                "bytes": len(source.lock_bytes),
                "sha256": hashlib.sha256(source.lock_bytes).hexdigest(),
                "documents": len(source.lock["documents"]),
            }
        )
    if len(documents) != CASE_COUNT:
        raise ValueError(
            f"full render corpus requires exactly 800 documents, got {len(documents)}"
        )
    documents.sort(key=lambda item: item["id"])
    source_records.sort(key=lambda item: item["campaign"])
    limits = {
        "max_documents": CASE_COUNT,
        "max_input_bytes": max(
            source.lock["limits"]["max_input_bytes"] for source in sources
        ),
        "max_total_input_bytes": sum(
            source.lock["limits"]["max_total_input_bytes"] for source in sources
        ),
        "max_pages_per_document": max(
            source.lock["limits"]["max_pages_per_document"] for source in sources
        ),
    }
    lock = {
        "schema": "rwml.render-full-corpus-lock.v1",
        "campaign": CAMPAIGN,
        "sources": source_records,
        "generator_closure_sha256": batch.generator_closure_sha256(
            ROOT, (Path(__file__).resolve(), Path(batch.__file__).resolve())
        ),
        "limits": limits,
        "documents": documents,
        "provenance": sorted(provenance.values(), key=lambda item: item["id"]),
        "coverage": {
            "case_count": CASE_COUNT,
            "source_batches": len(sources),
            "format_counts": {
                fmt: sum(d["format"] == fmt for d in documents)
                for fmt in ("doc", "docx")
            },
            "expected_pages": sum(d["expected"]["pages"] for d in documents),
            "cohort_counts": {s["campaign"]: s["documents"] for s in source_records},
            "design": "exact-union-of-locked-diagnostic-batches",
            "scope": "diagnostic-corpus-not-external-fidelity-acceptance",
        },
    }
    batch.validate_payloads(lock, payloads)
    return lock, payloads


def build_lock() -> dict[str, object]:
    return compose(source_batches())[0]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--refresh-lock", action="store_true")
    action.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    try:
        current, payloads = compose(source_batches())
        if args.refresh_lock:
            batch.atomic_write(args.lock, batch.canonical_json(current))
        else:
            lock = batch.load_lock(args.lock, current)
            if args.check:
                with tempfile.TemporaryDirectory() as tmp:
                    batch.materialize(Path(tmp) / "full", lock, current, payloads)
            else:
                print(batch.materialize(args.output, lock, current, payloads))
        return 0
    except (OSError, ValueError) as error:
        print(f"compose_render_full_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
