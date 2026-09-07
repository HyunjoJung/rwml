"""Shared identity and materialization operations for locked corpus batches."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import tempfile

try:
    from render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest
except ModuleNotFoundError:
    from scripts.render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def generator_closure_sha256(root: Path, paths: tuple[Path, ...]) -> str:
    digest = hashlib.sha256()
    for path in sorted(set(paths)):
        relative = path.relative_to(root).as_posix().encode("utf-8")
        payload = path.read_bytes()
        for part in (relative, payload):
            digest.update(len(part).to_bytes(8, "little"))
            digest.update(part)
    return digest.hexdigest()


def atomic_write(path: Path, payload: bytes) -> None:
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
        temporary.unlink(missing_ok=True)


def load_lock(path: Path, expected: dict[str, object]) -> dict[str, object]:
    if path.read_bytes() != canonical_json(expected):
        raise ValueError("render corpus lock is missing, noncanonical, or stale")
    return expected


def manifest_from_lock(lock: dict[str, object]) -> dict[str, object]:
    return {
        "schema": CORPUS_SCHEMA,
        "campaign": lock["campaign"],
        "limits": lock["limits"],
        "provenance": [
            {key: item[key] for key in ("id", "kind", "license", "reference")}
            for item in lock["provenance"]
        ],
        "documents": [
            {
                key: item[key]
                for key in (
                    "id",
                    "path",
                    "format",
                    "bytes",
                    "sha256",
                    "provenance",
                    "features",
                    "expected",
                )
            }
            for item in lock["documents"]
        ],
    }


def validate_payloads(lock: dict[str, object], payloads: dict[str, bytes]) -> None:
    records = [(item["path"], item) for item in lock["documents"]]
    records += [(item["reference"], item) for item in lock["provenance"]]
    if len(records) != len(payloads) or {path for path, _ in records} != set(payloads):
        raise ValueError("render corpus payload paths do not match the lock")
    for relative, record in records:
        path = PurePosixPath(relative)
        if (
            path.is_absolute()
            or path.as_posix() != relative
            or ".." in path.parts
            or "\\" in relative
            or ":" in relative
            or relative == "RENDER_ORACLE.json"
        ):
            raise ValueError(f"invalid render corpus payload path: {relative}")
        payload = payloads[relative]
        if (
            len(payload) != record["bytes"]
            or hashlib.sha256(payload).hexdigest() != record["sha256"]
        ):
            raise ValueError(f"render corpus payload identity mismatch: {relative}")


def materialize(
    output: Path,
    lock: dict[str, object],
    expected: dict[str, object],
    payloads: dict[str, bytes],
) -> Path:
    if canonical_json(lock) != canonical_json(expected):
        raise ValueError(
            "render corpus lock does not match the current generator closure"
        )
    if output.is_symlink() or (output.exists() and not output.is_dir()):
        raise ValueError(f"invalid render corpus output directory: {output}")
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"render corpus output directory must be fresh: {output}")
    validate_payloads(lock, payloads)
    # Publish the directory only after the full strict contract succeeds.
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix=f".{output.name}.", dir=output.parent
    ) as tmp:
        staged = Path(tmp) / "batch"
        staged.mkdir()
        for relative, payload in sorted(payloads.items()):
            atomic_write(staged / relative, payload)
        atomic_write(
            staged / "RENDER_ORACLE.json", canonical_json(manifest_from_lock(lock))
        )
        load_corpus_manifest(staged / "RENDER_ORACLE.json")
        if output.exists():
            output.rmdir()
        staged.rename(output)
    return output / "RENDER_ORACLE.json"
