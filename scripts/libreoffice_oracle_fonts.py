#!/usr/bin/env python3
"""Strict font provenance and PDF attestation for the local LibreOffice oracle."""

from __future__ import annotations

import hashlib
import json
import os
import re
import stat
import struct
from pathlib import Path, PurePosixPath
from typing import Any


SCHEMA = "rwml.libreoffice-oracle-font-lock.v1"
DEFAULT_FONT_LOCK = (
    Path(__file__).resolve().parents[1]
    / "corpus"
    / "public"
    / "oracle"
    / "libreoffice-font-lock.json"
)
MAX_LOCK_BYTES = 256 * 1024
MAX_FONT_BYTES = 64 * 1024 * 1024
MAX_ARCHIVE_BYTES = 256 * 1024 * 1024
MAX_FAMILIES = 32
MAX_FILES = 128
SHA256_RE = re.compile(r"[0-9a-f]{64}")
COMMIT_RE = re.compile(r"[0-9a-f]{40}")
SUBSET_PREFIX_RE = re.compile(r"^[A-Z]{6}\+")

LOCK_KEYS = {"schema", "license", "families"}
FAMILY_KEYS = {"family", "source", "files"}
SOURCE_KEYS = {"repository", "release_tag", "target_commit", "asset"}
ASSET_KEYS = {"name", "bytes", "sha256"}
FILE_KEYS = {
    "asset_member",
    "bytes",
    "name",
    "postscript_name",
    "sfnt_revision",
    "sha256",
    "style",
}


def _require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    if actual != expected:
        raise ValueError(
            f"{label} keys differ: "
            f"missing={sorted(expected - actual)}, extra={sorted(actual - expected)}"
        )


def _safe_text(value: object, label: str, maximum: int = 256) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > maximum
        or not value.isascii()
        or any(ord(character) < 0x20 for character in value)
    ):
        raise ValueError(f"{label} must be bounded printable ASCII")
    return value


def _positive_int(value: object, label: str, maximum: int) -> int:
    if (
        isinstance(value, bool)
        or not isinstance(value, int)
        or value <= 0
        or value > maximum
    ):
        raise ValueError(f"{label} is outside the contract")
    return value


def _sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise ValueError(f"{label} must be a lowercase SHA-256")
    return value


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _read_regular_file(path: Path, maximum: int) -> bytes:
    if path.is_symlink():
        raise ValueError(f"{path.name} must not be a symlink")
    descriptor: int | None = None
    try:
        descriptor = os.open(
            path,
            os.O_RDONLY
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_NONBLOCK", 0),
        )
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"{path.name} is not a regular file")
        if before.st_size <= 0 or before.st_size > maximum:
            raise ValueError(f"{path.name} size is outside the contract")
        payload = bytearray()
        while len(payload) < before.st_size:
            chunk = os.read(descriptor, min(1024 * 1024, before.st_size - len(payload)))
            if not chunk:
                break
            payload.extend(chunk)
        after = os.fstat(descriptor)
    except ValueError:
        raise
    except OSError as error:
        raise ValueError(f"{path.name} is unreadable") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
    if (
        len(payload) != before.st_size
        or before.st_dev != after.st_dev
        or before.st_ino != after.st_ino
        or before.st_size != after.st_size
        or before.st_mtime_ns != after.st_mtime_ns
    ):
        raise ValueError(f"{path.name} changed while being read")
    return bytes(payload)


def _validate_lock(value: dict[str, Any]) -> None:
    _require_exact_keys(value, LOCK_KEYS, "font lock")
    if value["schema"] != SCHEMA:
        raise ValueError(f"font lock schema must be {SCHEMA}")
    if value["license"] != "SIL-OFL-1.1":
        raise ValueError("font lock license must be SIL-OFL-1.1")
    families = value["families"]
    if not isinstance(families, list) or not 1 <= len(families) <= MAX_FAMILIES:
        raise ValueError("font lock families are outside the contract")

    family_names: list[str] = []
    filenames: set[str] = set()
    postscript_names: set[str] = set()
    total_files = 0
    for family in families:
        if not isinstance(family, dict):
            raise ValueError("font lock family must be an object")
        _require_exact_keys(family, FAMILY_KEYS, "font lock family")
        family_name = _safe_text(family["family"], "font family")
        family_names.append(family_name)

        source = family["source"]
        if not isinstance(source, dict):
            raise ValueError("font source must be an object")
        _require_exact_keys(source, SOURCE_KEYS, "font source")
        repository = _safe_text(source["repository"], "font source repository")
        if repository.count("/") != 1:
            raise ValueError("font source repository is invalid")
        _safe_text(source["release_tag"], "font source release tag")
        target_commit = source["target_commit"]
        if not isinstance(target_commit, str) or COMMIT_RE.fullmatch(target_commit) is None:
            raise ValueError("font source target commit is invalid")
        asset = source["asset"]
        if not isinstance(asset, dict):
            raise ValueError("font source asset must be an object")
        _require_exact_keys(asset, ASSET_KEYS, "font source asset")
        asset_name = _safe_text(asset["name"], "font source asset name")
        if PurePosixPath(asset_name).name != asset_name or not asset_name.endswith(".zip"):
            raise ValueError("font source asset name is invalid")
        _positive_int(asset["bytes"], "font source asset bytes", MAX_ARCHIVE_BYTES)
        _sha256(asset["sha256"], "font source asset SHA-256")

        files = family["files"]
        if not isinstance(files, list) or not files:
            raise ValueError("font family files must be a non-empty list")
        file_order: list[str] = []
        for entry in files:
            total_files += 1
            if total_files > MAX_FILES or not isinstance(entry, dict):
                raise ValueError("font files are outside the contract")
            _require_exact_keys(entry, FILE_KEYS, "font file")
            name = _safe_text(entry["name"], "font filename")
            if PurePosixPath(name).name != name or not name.endswith(".ttf"):
                raise ValueError("font filename is invalid")
            if name in filenames:
                raise ValueError(f"duplicate font filename: {name}")
            filenames.add(name)
            file_order.append(name)
            postscript = _safe_text(entry["postscript_name"], "font PostScript name")
            if postscript in postscript_names:
                raise ValueError(f"duplicate font PostScript name: {postscript}")
            postscript_names.add(postscript)
            _safe_text(entry["style"], "font style")
            _positive_int(entry["bytes"], "font bytes", MAX_FONT_BYTES)
            _positive_int(entry["sfnt_revision"], "font SFNT revision", 0xFFFFFFFF)
            _sha256(entry["sha256"], "font SHA-256")
            member = _safe_text(entry["asset_member"], "font asset member", 512)
            member_path = PurePosixPath(member)
            if (
                member_path.is_absolute()
                or ".." in member_path.parts
                or member_path.name != name
            ):
                raise ValueError("font asset member is invalid")
        if file_order != sorted(file_order):
            raise ValueError("font files must be sorted by filename")
    if family_names != sorted(family_names) or len(set(family_names)) != len(family_names):
        raise ValueError("font families must be unique and sorted")


def load_font_lock(path: Path = DEFAULT_FONT_LOCK) -> dict[str, Any]:
    payload = _read_regular_file(path, MAX_LOCK_BYTES)
    try:
        value = json.loads(
            payload,
            object_pairs_hook=_reject_duplicate_pairs,
            parse_constant=lambda token: (_ for _ in ()).throw(
                ValueError(f"non-finite JSON number: {token}")
            ),
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{path.name} is not strict UTF-8 JSON") from error
    if not isinstance(value, dict):
        raise ValueError("font lock must contain a JSON object")
    _validate_lock(value)
    return value


def font_files(lock: dict[str, Any]) -> list[dict[str, Any]]:
    return [entry for family in lock["families"] for entry in family["files"]]


def installation_font_identity(
    executable: Path, lock: dict[str, Any]
) -> list[dict[str, object]]:
    resolved = executable.resolve()
    roots = {resolved.parent.parent}
    for base in {executable.absolute().parent, resolved.parent}:
        try:
            applications = list(base.glob("*.app"))
        except OSError:
            applications = []
        for application in applications:
            contents = application / "Contents"
            if contents.is_dir():
                roots.add(contents.resolve())
    errors = []
    for installation_root in sorted(roots, key=lambda path: path.as_posix()):
        try:
            return _installation_font_identity_at_root(
                installation_root, lock
            )
        except ValueError as error:
            errors.append(str(error))
    raise ValueError(
        "LibreOffice font bundle does not match the public lock: "
        + (errors[-1] if errors else "installation root is unavailable")
    )


def _installation_font_identity_at_root(
    installation_root: Path, lock: dict[str, Any]
) -> list[dict[str, object]]:
    if not installation_root.is_dir():
        raise ValueError("installation root is unavailable")
    expected = {entry["name"]: entry for entry in font_files(lock)}
    matches: dict[str, list[Path]] = {name: [] for name in expected}
    try:
        for candidate in installation_root.rglob("*.ttf"):
            if candidate.name in matches:
                matches[candidate.name].append(candidate)
    except OSError as error:
        raise ValueError("LibreOffice font bundle could not be inspected") from error

    identity = []
    for name in sorted(expected):
        candidates = matches[name]
        if len(candidates) != 1:
            raise ValueError(f"LibreOffice font bundle must contain one {name}")
        payload = _read_regular_file(candidates[0], MAX_FONT_BYTES)
        entry = expected[name]
        if (
            len(payload) != entry["bytes"]
            or hashlib.sha256(payload).hexdigest() != entry["sha256"]
        ):
            raise ValueError(f"LibreOffice font bundle identity differs: {name}")
        identity.append(
            {
                "bytes": entry["bytes"],
                "name": name,
                "postscript_name": entry["postscript_name"],
                "sfnt_revision": entry["sfnt_revision"],
                "sha256": entry["sha256"],
            }
        )
    return identity


def sfnt_revision(payload: bytes) -> int:
    if len(payload) < 12 or payload[:4] not in {b"\x00\x01\x00\x00", b"true"}:
        raise ValueError("embedded font is not bounded TrueType SFNT data")
    table_count = struct.unpack(">H", payload[4:6])[0]
    if table_count < 1 or table_count > 256 or 12 + table_count * 16 > len(payload):
        raise ValueError("embedded font has no valid head table directory")
    for index in range(table_count):
        record = 12 + index * 16
        if payload[record : record + 4] != b"head":
            continue
        offset, length = struct.unpack(">II", payload[record + 8 : record + 16])
        if length < 8 or offset > len(payload) - length:
            raise ValueError("embedded font head table is out of bounds")
        return struct.unpack(">I", payload[offset + 4 : offset + 8])[0]
    raise ValueError("embedded font has no head table")


def normalized_postscript_name(value: str) -> str:
    return SUBSET_PREFIX_RE.sub("", value, count=1)


def validate_pdf_font_identities(
    identities: list[dict[str, object]], lock: dict[str, Any]
) -> None:
    if not identities:
        raise ValueError("reference PDF has no embedded fonts")
    expected = {entry["postscript_name"]: entry for entry in font_files(lock)}
    for identity in identities:
        if set(identity) != {"postscript_name", "sfnt_revision"}:
            raise ValueError("reference PDF font identity is malformed")
        name = identity["postscript_name"]
        revision = identity["sfnt_revision"]
        if not isinstance(name, str) or name not in expected:
            raise ValueError(f"reference PDF font is not locked: {name}")
        if (
            isinstance(revision, bool)
            or not isinstance(revision, int)
            or revision != expected[name]["sfnt_revision"]
        ):
            raise ValueError(f"reference PDF font revision differs: {name}")
