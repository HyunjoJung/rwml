#!/usr/bin/env python3
"""Prepare and verify an offline, digest-locked diagnostic font source pack.

Source identity does not attest embedded PDF subsets or rendering fidelity.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import struct
import sys

from libreoffice_oracle_fonts import (
    DEFAULT_FONT_LOCK,
    MAX_FILES,
    MAX_FONT_BYTES,
    MAX_LOCK_BYTES,
    _positive_int,
    _read_regular_file,
    _require_exact_keys,
    _safe_text,
    _sha256,
    _validate_lock as validate_base_lock,
    font_files,
    sfnt_revision,
)
from render_oracle_contract import _assert_path_neutral, _load_json

DEFAULT_LOCK = DEFAULT_FONT_LOCK.with_name("shared-font-lock.json")
SCHEMA = "rwml.libreoffice-oracle-font-lock.v2"
PACK_SCHEMA = "rwml.shared-oracle-font-pack.v1"
MAX_LICENSE_BYTES = 1024 * 1024
MAX_TOTAL_BYTES = 256 * 1024 * 1024
IDENTITY_KEYS = {
    "name",
    "bytes",
    "sha256",
    "postscript_name",
    "sfnt_revision",
    "style",
    "format",
}


@dataclass(frozen=True)
class SharedFontLock:
    sha256: str
    base_sha256: str
    fonts: tuple[dict, ...]
    licenses: tuple[dict, ...]


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, separators=(",", ":"), allow_nan=False
    ).encode()


def sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def git_blob_sha1(payload: bytes) -> str:
    return hashlib.sha1(
        b"blob " + str(len(payload)).encode() + b"\0" + payload
    ).hexdigest()


def _object(value: object, keys: set[str], label: str) -> dict:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    _require_exact_keys(value, keys, label)
    return value


def _path(value: object, label: str, *, filename: bool = False) -> str:
    text = _safe_text(value, label, 512)
    path = PurePosixPath(text)
    if (
        path.is_absolute()
        or str(path) != text
        or ".." in path.parts
        or any(character in text for character in '\\:\x7f<>"|?*')
        or any(part in {".", ".."} or part.endswith((".", " ")) for part in path.parts)
        or any(
            re.fullmatch(
                r"CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9]|CONIN\$|CONOUT\$",
                part.split(".")[0].rstrip().upper(),
            )
            for part in path.parts
        )
        or not path.parts
        or (filename and len(path.parts) != 1)
    ):
        raise ValueError(f"{label} is not a canonical relative path")
    return text


def _git_sha(value: object, label: str) -> None:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{40}", value) is None:
        raise ValueError(f"{label} must be a full lowercase Git SHA")


def load_lock(
    path: Path = DEFAULT_LOCK, base_path: Path = DEFAULT_FONT_LOCK
) -> SharedFontLock:
    document, payload = _load_json(path, MAX_LOCK_BYTES)
    _require_exact_keys(
        document,
        {"schema", "license", "base_lock", "additions", "font_order"},
        "shared font lock",
    )
    if document["schema"] != SCHEMA or document["license"] != "SIL-OFL-1.1":
        raise ValueError("shared font schema or license differs")
    base_ref = _object(document["base_lock"], {"name", "sha256"}, "base lock reference")
    _path(base_ref["name"], "base lock name", filename=True)
    if base_ref["name"] != base_path.name:
        raise ValueError("base lock name differs")
    _sha256(base_ref["sha256"], "base lock digest")
    base, base_payload = _load_json(base_path, MAX_LOCK_BYTES)
    if sha256(base_payload) != base_ref["sha256"]:
        raise ValueError("base lock digest differs")
    validate_base_lock(base)
    _assert_path_neutral(document, "shared font lock")
    _assert_path_neutral(base, "base font lock")
    entries = {
        entry["name"]: {key: entry[key] for key in IDENTITY_KEYS - {"format"}}
        | {"format": "truetype"}
        for entry in font_files(base)
    }
    names = {name.casefold() for name in entries}
    if len(names) != len(entries):
        raise ValueError("base font filenames are aliased")
    for name in entries:
        _path(name, "base font name", filename=True)
    postscript = {entry["postscript_name"] for entry in entries.values()}
    licenses = {}
    additions = document["additions"]
    if not isinstance(additions, list) or not 1 <= len(additions) <= 32:
        raise ValueError("shared font additions are outside the contract")
    addition_names = []
    for entry in additions:
        _object(entry, IDENTITY_KEYS | {"family", "source"}, "shared font")
        name = _path(entry["name"], "font name", filename=True)
        format_name = entry["format"]
        if format_name not in ("truetype", "opentype-cff", "truetype-variable"):
            raise ValueError("font source format is unsupported")
        if not name.endswith(".otf" if format_name == "opentype-cff" else ".ttf"):
            raise ValueError("font source extension differs")
        if name.casefold() in names:
            raise ValueError("font filename is duplicated or aliased")
        names.add(name.casefold())
        addition_names.append(name)
        for key in ("family", "style", "postscript_name"):
            _safe_text(entry[key], f"font {key}")
        if entry["postscript_name"] in postscript:
            raise ValueError("font PostScript identity is duplicated")
        postscript.add(entry["postscript_name"])
        _positive_int(entry["bytes"], "font bytes", MAX_FONT_BYTES)
        _positive_int(entry["sfnt_revision"], "font revision", 0xFFFFFFFF)
        _sha256(entry["sha256"], "font digest")
        source = _object(
            entry["source"],
            {"kind", "repository", "target_commit", "font", "license"},
            "font source",
        )
        if source["kind"] != "github-blob":
            raise ValueError("font source kind differs")
        repository = _path(source["repository"], "source repository")
        if re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is None:
            raise ValueError("source repository is invalid")
        _git_sha(source["target_commit"], "source commit")
        raw = _object(source["font"], {"path", "git_blob_sha1"}, "font blob")
        if PurePosixPath(_path(raw["path"], "font source path")).name != name:
            raise ValueError("font source basename differs")
        _git_sha(raw["git_blob_sha1"], "font blob")
        license_entry = _object(
            source["license"],
            {"name", "path", "bytes", "sha256", "git_blob_sha1"},
            "font license",
        )
        license_name = _path(license_entry["name"], "license name", filename=True)
        if not license_name.endswith(".txt") or license_name.casefold() in licenses:
            raise ValueError("license filename is invalid or aliased")
        _path(license_entry["path"], "license source path")
        _positive_int(license_entry["bytes"], "license bytes", MAX_LICENSE_BYTES)
        _sha256(license_entry["sha256"], "license digest")
        _git_sha(license_entry["git_blob_sha1"], "license blob")
        licenses[license_name.casefold()] = {
            key: license_entry[key]
            for key in ("name", "bytes", "sha256", "git_blob_sha1")
        }
        entries[name] = {key: entry[key] for key in IDENTITY_KEYS} | {
            "git_blob_sha1": raw["git_blob_sha1"]
        }
    if addition_names != sorted(addition_names):
        raise ValueError("shared additions must be sorted by filename")
    order = document["font_order"]
    if (
        not isinstance(order, list)
        or not all(isinstance(name, str) for name in order)
        or len(order) != len(entries)
        or len(order) > MAX_FILES
        or set(order) != set(entries)
    ):
        raise ValueError("font order does not identify every font exactly once")
    if (
        sum(entry["bytes"] for entry in [*entries.values(), *licenses.values()])
        > MAX_TOTAL_BYTES
    ):
        raise ValueError("shared font pack exceeds the aggregate byte bound")
    return SharedFontLock(
        sha256(payload),
        sha256(base_payload),
        tuple(entries[name] for name in order),
        tuple(sorted(licenses.values(), key=lambda entry: entry["name"])),
    )


def _directory(path: Path, expected: set[str]) -> None:
    if path.is_symlink() or not path.is_dir():
        raise ValueError("pack directory is unavailable or symlinked")
    seen = set()
    for entry in path.iterdir():
        if entry.name not in expected or len(seen) >= len(expected):
            raise ValueError("pack file set differs")
        seen.add(entry.name)
    if seen != expected:
        raise ValueError("pack file set differs")


def _source_revision(payload: bytes, format_name: str) -> int:
    # OTTO is allowed only for locked source metadata, never the PDF TTF verifier.
    if format_name == "opentype-cff":
        if payload[:4] != b"OTTO":
            raise ValueError("source CFF font signature differs")
        payload = b"\x00\x01\x00\x00" + payload[4:]
    elif payload[:4] not in {b"\x00\x01\x00\x00", b"true"}:
        raise ValueError("source TrueType signature differs")
    revision = sfnt_revision(payload)
    count = struct.unpack_from(">H", payload, 4)[0]
    tables = {payload[12 + index * 16 : 16 + index * 16] for index in range(count)}
    if format_name == "opentype-cff" and (b"CFF " not in tables or b"CFF2" in tables):
        raise ValueError("source CFF table declaration differs")
    if ("variable" in format_name) != (b"fvar" in tables):
        raise ValueError("source variable-font declaration differs")
    return revision


def _read_inputs(
    directory: Path, entries: tuple[dict, ...], maximum: int
) -> dict[str, bytes]:
    _directory(directory, {entry["name"] for entry in entries})
    payloads = {}
    for entry in entries:
        payload = _read_regular_file(
            directory / entry["name"], min(maximum, entry["bytes"])
        )
        if len(payload) != entry["bytes"] or sha256(payload) != entry["sha256"]:
            raise ValueError(f"{entry['name']} source identity differs")
        if (
            "git_blob_sha1" in entry
            and git_blob_sha1(payload) != entry["git_blob_sha1"]
        ):
            raise ValueError(f"{entry['name']} Git blob differs")
        if (
            "format" in entry
            and _source_revision(payload, entry["format"]) != entry["sfnt_revision"]
        ):
            raise ValueError(f"{entry['name']} source revision differs")
        payloads[entry["name"]] = payload
    return payloads


def _receipt(lock: SharedFontLock) -> dict:
    return {
        "schema": PACK_SCHEMA,
        "lock_sha256": lock.sha256,
        "base_lock_sha256": lock.base_sha256,
        "fonts": list(lock.fonts),
        "licenses": list(lock.licenses),
    }


def verify_pack(directory: Path, lock: SharedFontLock) -> dict:
    _directory(directory, {"fonts", "licenses", "MANIFEST.json"})
    _read_inputs(directory / "fonts", lock.fonts, MAX_FONT_BYTES)
    _read_inputs(directory / "licenses", lock.licenses, MAX_LICENSE_BYTES)
    receipt, _ = _load_json(directory / "MANIFEST.json", MAX_LOCK_BYTES)
    expected = _receipt(lock)
    if canonical_json(receipt) != canonical_json(expected):
        raise ValueError("font pack receipt differs")
    return expected


def prepare_pack(
    font_directory: Path, license_directory: Path, output: Path, lock: SharedFontLock
) -> dict:
    if output.exists() or output.is_symlink():
        raise ValueError("font pack output must be fresh")
    if any(
        output.resolve().is_relative_to(path.resolve())
        for path in (font_directory, license_directory)
    ):
        raise ValueError("font pack output overlaps its input directory")
    font_payloads = _read_inputs(font_directory, lock.fonts, MAX_FONT_BYTES)
    license_payloads = _read_inputs(license_directory, lock.licenses, MAX_LICENSE_BYTES)
    output.mkdir(parents=True, exist_ok=False)
    for name, payloads in (("fonts", font_payloads), ("licenses", license_payloads)):
        directory = output / name
        directory.mkdir()
        for filename, payload in payloads.items():
            with (directory / filename).open("xb") as stream:
                stream.write(payload)
    with (output / "MANIFEST.json").open("xb") as stream:
        stream.write(canonical_json(_receipt(lock)) + b"\n")
    return verify_pack(output, lock)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    parser.add_argument("--base-lock", type=Path, default=DEFAULT_FONT_LOCK)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--font-dir", type=Path, required=True)
    prepare.add_argument("--license-dir", type=Path, required=True)
    prepare.add_argument("--output", type=Path, required=True)
    verify = commands.add_parser("verify")
    verify.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        lock = load_lock(args.lock, args.base_lock)
        result = (
            prepare_pack(args.font_dir, args.license_dir, args.output, lock)
            if args.command == "prepare"
            else verify_pack(args.output, lock)
        )
        print(
            json.dumps(
                {
                    "fonts": len(lock.fonts),
                    "licenses": len(lock.licenses),
                    "manifest_sha256": sha256(canonical_json(result)),
                },
                sort_keys=True,
            )
        )
        return 0
    except (OSError, ValueError) as error:
        print(f"shared_oracle_fonts: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
