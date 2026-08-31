#!/usr/bin/env python3
"""Bounded, path-neutral identities for render-analysis runtimes."""

from __future__ import annotations

import hashlib
from importlib import metadata
import json
import os
import platform
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import sysconfig
from typing import Mapping

from libreoffice_oracle_fonts import _read_regular_file


SCHEMA = "rwml.analysis-tools.v1"
MAX_DISTRIBUTIONS = 16
MAX_ROOTS = 512
MAX_FILES = 100_000
MAX_DIRECTORIES = 100_000
MAX_DEPTH = 64
MAX_FILE_BYTES = 256 * 1024 * 1024
MAX_TOTAL_BYTES = 1024 * 1024 * 1024
MAX_PYTHON_FILES = 4 * MAX_FILES + 2
MAX_PYTHON_BYTES = 4 * MAX_TOTAL_BYTES + 2 * MAX_FILE_BYTES
NAME_RE = re.compile(r"[a-z0-9][a-z0-9._-]{0,63}")
SHA256_RE = re.compile(r"[0-9a-f]{64}")
IGNORED_INSTALLER_FILES = {"INSTALLER", "RECORD", "REQUESTED", "direct_url.json"}
PYTHON_FLAG_NAMES = (
    "debug",
    "dev_mode",
    "dont_write_bytecode",
    "hash_randomization",
    "ignore_environment",
    "int_max_str_digits",
    "isolated",
    "no_site",
    "no_user_site",
    "optimize",
    "safe_path",
    "utf8_mode",
)
LIMITS = {
    "max_distributions": MAX_DISTRIBUTIONS,
    "max_roots_per_closure": MAX_ROOTS,
    "max_files_per_closure": MAX_FILES,
    "max_directories_per_closure": MAX_DIRECTORIES,
    "max_depth": MAX_DEPTH,
    "max_file_bytes": MAX_FILE_BYTES,
    "max_total_bytes_per_closure": MAX_TOTAL_BYTES,
    "max_python_files": MAX_PYTHON_FILES,
    "max_python_bytes": MAX_PYTHON_BYTES,
}


def _canonical(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, ensure_ascii=True, separators=(",", ":"), allow_nan=False
    ).encode()


def _safe_text(value: object, label: str) -> str:
    if (
        not isinstance(value, str)
        or not value
        or len(value) > 128
        or any(ord(character) < 32 or ord(character) == 127 for character in value)
    ):
        raise ValueError(f"{label} is invalid")
    return value


def _normalized_distribution_name(value: str) -> str:
    return re.sub(r"[-_.]+", "-", value).lower()


def _relative_parts(value: object) -> tuple[str, ...] | None:
    raw = str(value).replace("\\", "/")
    path = PurePosixPath(raw)
    if path.is_absolute() or not path.parts:
        raise ValueError("distribution path is invalid")
    if ".." in path.parts:
        # Wheels legitimately record generated virtual-environment entry points
        # outside site-packages. They are path-local launchers, not import payload.
        return None
    if any(
        part in {"", "."}
        or any(ord(character) < 32 or ord(character) == 127 for character in part)
        for part in path.parts
    ):
        raise ValueError("distribution path is invalid")
    return path.parts


def _ignored(relative: PurePosixPath) -> bool:
    return (
        any(part.endswith(".dist-info") for part in relative.parts[:-1])
        and relative.name in IGNORED_INSTALLER_FILES
    )


def _inventory(
    root: Path, relative_roots: tuple[PurePosixPath, ...]
) -> list[tuple[PurePosixPath, tuple[int, int, int, int, int]]]:
    files: list[tuple[PurePosixPath, tuple[int, int, int, int, int]]] = []
    directories = 0

    def visit(path: Path, relative: PurePosixPath, depth: int) -> None:
        nonlocal directories
        if depth > MAX_DEPTH:
            raise ValueError("analysis payload depth exceeds its bound")
        if _ignored(relative):
            return
        try:
            status = path.lstat()
        except OSError as error:
            raise ValueError("analysis payload is unreadable") from error
        if stat.S_ISLNK(status.st_mode):
            raise ValueError("analysis payload contains a symlink")
        if stat.S_ISREG(status.st_mode):
            files.append(
                (
                    relative,
                    (
                        status.st_dev,
                        status.st_ino,
                        status.st_size,
                        status.st_mtime_ns,
                        status.st_ctime_ns,
                    ),
                )
            )
            if len(files) > MAX_FILES:
                raise ValueError("analysis payload file count exceeds its bound")
            return
        if not stat.S_ISDIR(status.st_mode):
            raise ValueError("analysis payload contains a non-regular entry")
        directories += 1
        if directories > MAX_DIRECTORIES:
            raise ValueError("analysis payload directory count exceeds its bound")
        try:
            with os.scandir(path) as entries:
                children = sorted(entries, key=lambda entry: entry.name)
        except OSError as error:
            raise ValueError("analysis payload directory is unreadable") from error
        for child in children:
            visit(Path(child.path), relative / child.name, depth + 1)

    for relative in relative_roots:
        visit(root.joinpath(*relative.parts), relative, 1)
    return files


def _closure_identity(root: Path, roots: set[str], label: str) -> dict[str, int | str]:
    if not 1 <= len(roots) <= MAX_ROOTS:
        raise ValueError(f"{label} root count is outside the contract")
    try:
        root = root.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{label} root is unavailable") from error
    relative_roots = tuple(PurePosixPath(name) for name in sorted(roots))
    for relative in relative_roots:
        if len(relative.parts) != 1 or _relative_parts(relative) is None:
            raise ValueError(f"{label} root name is invalid")

    inventory = _inventory(root, relative_roots)
    if not inventory:
        raise ValueError(f"{label} payload is empty")
    digest = hashlib.sha256()
    total = 0
    for relative, _ in inventory:
        payload = _read_regular_file(
            root.joinpath(*relative.parts), MAX_FILE_BYTES, allow_empty=True
        )
        total += len(payload)
        if total > MAX_TOTAL_BYTES:
            raise ValueError(f"{label} payload exceeds its aggregate byte bound")
        encoded = relative.as_posix().encode()
        digest.update(len(encoded).to_bytes(8, "big"))
        digest.update(encoded)
        digest.update(len(payload).to_bytes(8, "big"))
        digest.update(payload)
    if inventory != _inventory(root, relative_roots):
        raise ValueError(f"{label} file set changed while being read")
    return {"files": len(inventory), "bytes": total, "sha256": digest.hexdigest()}


def _single_file_identity(path: Path, label: str) -> dict[str, int | str]:
    try:
        resolved = path.resolve(strict=True)
    except OSError as error:
        raise ValueError(f"{label} is unavailable") from error
    payload = _read_regular_file(resolved, MAX_FILE_BYTES, allow_empty=False)
    return {
        "files": 1,
        "bytes": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }


def distribution_identity(
    name: str,
    distribution_name: str,
    expected_version: str,
    imported_module: object,
) -> dict[str, int | str]:
    if NAME_RE.fullmatch(name) is None:
        raise ValueError("analysis distribution name is invalid")
    _safe_text(distribution_name, "analysis distribution package name")
    _safe_text(expected_version, "analysis distribution version")
    try:
        installed = metadata.distribution(distribution_name)
    except metadata.PackageNotFoundError as error:
        raise ValueError(f"analysis distribution is unavailable: {name}") from error
    installed_name = installed.metadata.get("Name")
    if not isinstance(installed_name, str) or _normalized_distribution_name(
        installed_name
    ) != _normalized_distribution_name(distribution_name):
        raise ValueError(f"analysis distribution name differs: {name}")
    if str(installed.version) != expected_version:
        raise ValueError(f"analysis distribution version differs: {name}")
    if installed.files is None:
        raise ValueError(
            f"analysis distribution has no installed-file inventory: {name}"
        )
    roots: set[str] = set()
    for item in installed.files:
        parts = _relative_parts(item)
        if parts is not None:
            roots.add(parts[0])
    origin = getattr(imported_module, "__file__", None)
    if not isinstance(origin, (str, os.PathLike)):
        raise ValueError(f"analysis distribution import origin is unavailable: {name}")
    origin_path = Path(origin)
    if origin_path.is_symlink():
        raise ValueError(f"analysis distribution import origin is a symlink: {name}")
    try:
        root = Path(installed.locate_file("")).resolve(strict=True)
        relative_origin = origin_path.resolve(strict=True).relative_to(root)
    except (OSError, ValueError) as error:
        raise ValueError(
            f"analysis distribution import origin differs: {name}"
        ) from error
    relative = PurePosixPath(*relative_origin.parts)
    if (
        not relative.parts
        or relative.parts[0] not in roots
        or relative.parts[0].endswith(".dist-info")
        or _ignored(relative)
    ):
        raise ValueError(f"analysis distribution import origin differs: {name}")
    payload = _closure_identity(root, roots, f"analysis distribution {name}")
    return {"name": name, "version": expected_version, **payload}


def _python_library() -> Path | None:
    directory = sysconfig.get_config_var("LIBDIR")
    if not isinstance(directory, str) or not directory:
        return None
    for key in ("LDLIBRARY", "INSTSONAME"):
        name = sysconfig.get_config_var(key)
        if isinstance(name, str) and name:
            candidate = Path(directory) / name
            if candidate.exists():
                return candidate
    return None


def python_identity() -> dict[str, object]:
    executable = _single_file_identity(Path(sys.executable), "Python executable")
    components = {"executable": executable}
    seen: set[Path] = set()
    closures = (
        ("standard_library", "stdlib", {"site-packages", "dist-packages"}),
        ("platform_library", "platstdlib", {"site-packages", "dist-packages"}),
        ("site_packages", "purelib", set()),
        ("platform_site_packages", "platlib", set()),
    )
    for component, path_name, excluded in closures:
        configured = sysconfig.get_path(path_name)
        if not isinstance(configured, str) or not configured:
            raise ValueError(f"Python {path_name} path is unavailable")
        root = Path(configured)
        try:
            resolved = root.resolve(strict=True)
            roots = {
                child.name for child in root.iterdir() if child.name not in excluded
            }
        except OSError as error:
            raise ValueError(f"Python {path_name} path is unavailable") from error
        if resolved in seen:
            continue
        seen.add(resolved)
        if not roots:
            if path_name == "stdlib":
                raise ValueError("Python standard library payload is empty")
            continue
        components[component] = _closure_identity(root, roots, f"Python {path_name}")
    library = _python_library()
    if library is not None:
        components["shared_library"] = _single_file_identity(
            library, "Python shared library"
        )
    payload = {
        "files": sum(int(item["files"]) for item in components.values()),
        "bytes": sum(int(item["bytes"]) for item in components.values()),
        "sha256": hashlib.sha256(_canonical(components)).hexdigest(),
    }
    return {
        "implementation": _safe_text(
            platform.python_implementation(), "Python implementation"
        ),
        "version": _safe_text(platform.python_version(), "Python version"),
        "cache_tag": _safe_text(
            sys.implementation.cache_tag or "unavailable", "Python cache tag"
        ),
        "abi": _safe_text(
            str(sysconfig.get_config_var("SOABI") or "unavailable"), "Python ABI"
        ),
        "platform": _safe_text(sysconfig.get_platform(), "Python platform"),
        "flags": {name: int(getattr(sys.flags, name)) for name in PYTHON_FLAG_NAMES},
        **payload,
    }


def analysis_identity(
    distributions: Mapping[str, tuple[str, str, object]],
) -> dict[str, object]:
    if (
        not isinstance(distributions, Mapping)
        or not 1 <= len(distributions) <= MAX_DISTRIBUTIONS
    ):
        raise ValueError("analysis distribution count is outside the contract")
    packages = []
    for name in sorted(distributions):
        value = distributions[name]
        if (
            not isinstance(value, tuple)
            or len(value) != 3
            or not all(isinstance(item, str) for item in value[:2])
        ):
            raise ValueError("analysis distribution declaration is invalid")
        packages.append(distribution_identity(name, value[0], value[1], value[2]))
    body = {
        "schema": SCHEMA,
        "limits": dict(LIMITS),
        "python": python_identity(),
        "distributions": packages,
    }
    result = {
        **body,
        "identity_sha256": hashlib.sha256(_canonical(body)).hexdigest(),
    }
    validate_analysis_identity(result)
    return result


def _validate_payload(
    value: object,
    keys: set[str],
    label: str,
    *,
    max_files: int,
    max_bytes: int,
) -> None:
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError(f"{label} has invalid fields")
    if type(value["files"]) is not int or not 1 <= value["files"] <= max_files:
        raise ValueError(f"{label} file count is invalid")
    if type(value["bytes"]) is not int or not 1 <= value["bytes"] <= max_bytes:
        raise ValueError(f"{label} byte count is invalid")
    if (
        not isinstance(value["sha256"], str)
        or SHA256_RE.fullmatch(value["sha256"]) is None
    ):
        raise ValueError(f"{label} digest is invalid")


def validate_analysis_identity(value: object) -> None:
    keys = {"schema", "limits", "python", "distributions", "identity_sha256"}
    if not isinstance(value, dict) or set(value) != keys:
        raise ValueError("analysis identity has invalid fields")
    if value["schema"] != SCHEMA or value["limits"] != LIMITS:
        raise ValueError("analysis identity schema or limits differ")
    python = value["python"]
    python_keys = {
        "implementation",
        "version",
        "cache_tag",
        "abi",
        "platform",
        "flags",
        "files",
        "bytes",
        "sha256",
    }
    _validate_payload(
        python,
        python_keys,
        "Python identity",
        max_files=MAX_PYTHON_FILES,
        max_bytes=MAX_PYTHON_BYTES,
    )
    for key in ("implementation", "version", "cache_tag", "abi", "platform"):
        _safe_text(python[key], f"Python {key}")
    flags = python["flags"]
    if (
        not isinstance(flags, dict)
        or set(flags) != set(PYTHON_FLAG_NAMES)
        or any(type(item) is not int or item < 0 for item in flags.values())
    ):
        raise ValueError("Python flags are invalid")
    distributions = value["distributions"]
    if (
        not isinstance(distributions, list)
        or not 1 <= len(distributions) <= MAX_DISTRIBUTIONS
    ):
        raise ValueError("analysis identity distributions are invalid")
    names = []
    for package in distributions:
        _validate_payload(
            package,
            {"name", "version", "files", "bytes", "sha256"},
            "distribution identity",
            max_files=MAX_FILES,
            max_bytes=MAX_TOTAL_BYTES,
        )
        name = package["name"]
        if not isinstance(name, str) or NAME_RE.fullmatch(name) is None:
            raise ValueError("analysis distribution name is invalid")
        _safe_text(package["version"], "analysis distribution version")
        names.append(name)
    if names != sorted(set(names)):
        raise ValueError("analysis distributions must be unique and sorted")
    body = {key: value[key] for key in ("schema", "limits", "python", "distributions")}
    expected = hashlib.sha256(_canonical(body)).hexdigest()
    if value["identity_sha256"] != expected:
        raise ValueError("analysis identity digest differs")


def tool_versions(value: object) -> dict[str, str]:
    validate_analysis_identity(value)
    assert isinstance(value, dict)
    python = value["python"]
    distributions = value["distributions"]
    assert isinstance(python, dict) and isinstance(distributions, list)
    return {
        **{item["name"]: item["version"] for item in distributions},
        "python": python["version"],
    }
