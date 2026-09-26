#!/usr/bin/env python3
"""Apply fixed POSIX resource limits, then replace this process with a command."""

from __future__ import annotations

import argparse
import os
import sys

try:
    import resource
except ModuleNotFoundError:  # pragma: no cover - exercised by Windows CI import.
    resource = None  # type: ignore[assignment]


MAX_CPU_SECONDS = 60 * 60
MAX_FILE_BYTES = 1024 * 1024 * 1024
MAX_OPEN_FILES = 4096
MAX_PROCESSES = 1024
MAX_CORE_BYTES = 1024 * 1024 * 1024
MAX_ADDRESS_SPACE_BYTES = 64 * 1024 * 1024 * 1024


def _bounded_integer(label: str, minimum: int, maximum: int):
    def parse(value: str) -> int:
        try:
            result = int(value, 10)
        except ValueError as error:
            raise argparse.ArgumentTypeError(f"{label} must be an integer") from error
        if not minimum <= result <= maximum:
            raise argparse.ArgumentTypeError(f"{label} is outside the contract")
        return result

    return parse


def _set_limit(key: int, value: int, label: str) -> None:
    assert resource is not None
    try:
        _, hard = resource.getrlimit(key)
        if hard != resource.RLIM_INFINITY and value > hard:
            raise ValueError(f"{label} exceeds the inherited hard limit")
        expected = (value, value)
        resource.setrlimit(key, expected)
        if resource.getrlimit(key) != expected:
            raise ValueError(f"{label} was not applied exactly")
    except (OSError, ValueError) as error:
        if isinstance(error, ValueError) and str(error).startswith(label):
            raise
        raise ValueError(f"{label} could not be applied") from error


def apply_limits(
    *,
    cpu_seconds: int,
    file_bytes: int,
    open_files: int,
    processes: int,
    core_bytes: int,
    address_space_bytes: int | None,
) -> None:
    if os.name != "posix" or resource is None:
        raise ValueError("POSIX resource limits are unavailable")
    _set_limit(resource.RLIMIT_CORE, core_bytes, "core size limit")
    _set_limit(resource.RLIMIT_CPU, cpu_seconds, "CPU time limit")
    _set_limit(resource.RLIMIT_FSIZE, file_bytes, "file size limit")
    _set_limit(resource.RLIMIT_NOFILE, open_files, "open file limit")
    _set_limit(resource.RLIMIT_NPROC, processes, "process count limit")
    if address_space_bytes is not None:
        _set_limit(resource.RLIMIT_AS, address_space_bytes, "address space limit")


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--cpu-seconds",
        required=True,
        type=_bounded_integer("CPU seconds", 1, MAX_CPU_SECONDS),
    )
    parser.add_argument(
        "--file-bytes",
        required=True,
        type=_bounded_integer("file bytes", 1, MAX_FILE_BYTES),
    )
    parser.add_argument(
        "--open-files",
        required=True,
        type=_bounded_integer("open files", 16, MAX_OPEN_FILES),
    )
    parser.add_argument(
        "--processes",
        required=True,
        type=_bounded_integer("processes", 1, MAX_PROCESSES),
    )
    parser.add_argument(
        "--core-bytes",
        required=True,
        type=_bounded_integer("core bytes", 0, MAX_CORE_BYTES),
    )
    parser.add_argument(
        "--address-space-bytes",
        type=_bounded_integer(
            "address space bytes", 256 * 1024 * 1024, MAX_ADDRESS_SPACE_BYTES
        ),
    )
    parser.add_argument("command", nargs=argparse.REMAINDER)
    result = parser.parse_args(argv)
    if result.command[:1] == ["--"]:
        result.command = result.command[1:]
    if (
        not result.command
        or not os.path.isabs(result.command[0])
        or any(
            "\x00" in item or "\n" in item or "\r" in item for item in result.command
        )
    ):
        parser.error("command must begin with a safe absolute executable path")
    return result


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    try:
        apply_limits(
            cpu_seconds=args.cpu_seconds,
            file_bytes=args.file_bytes,
            open_files=args.open_files,
            processes=args.processes,
            core_bytes=args.core_bytes,
            address_space_bytes=args.address_space_bytes,
        )
        os.execv(args.command[0], args.command)
    except (OSError, ValueError) as error:
        print(f"bounded execution failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
