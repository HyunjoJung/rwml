#!/usr/bin/env python3
"""Digest-bound, resource-limited LibreOffice diagnostic container execution."""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import selectors
import signal
import subprocess
import sys
import tarfile
import time
import uuid

from libreoffice_oracle_fonts import _read_regular_file as read_regular_file
from render_oracle_contract import _load_json, _require_exact_keys, _require_sha256

ROOT = Path(__file__).resolve().parents[1]
RUNTIME = ROOT / "scripts" / "libreoffice-container"
DEFAULT_LOCK = ROOT / "corpus/public/oracle/libreoffice-container-lock.json"
SCHEMA = "rwml.libreoffice-container.v1"
ARCHIVE_URL = "https://downloadarchive.documentfoundation.org/libreoffice/old/26.2.3.2/deb/x86_64/LibreOffice_26.2.3.2_Linux_x86-64_deb.tar.gz"
VERSION_LINE = "LibreOffice 26.2.3.2 70e089b17412e4cb7773e41413306b17a2328c34"
RECIPE_FILES = {
    "Containerfile": RUNTIME / "Containerfile",
    "capture.sh": RUNTIME / "capture.sh",
    "fonts.conf": RUNTIME / "fonts.conf",
    "profile.xcu": ROOT / "scripts/render-oracle-local-profile.xcu",
}
MAX_PDF_BYTES = 64 * 1024 * 1024
MAX_CAPTURE_BYTES = MAX_PDF_BYTES + 1024 * 1024
CAPTURE_MEMBERS = {
    "output.pdf",
    "version.txt",
    "fonts.txt",
    "sha256.txt",
    "warmup.log",
    "conversion.log",
}
DIGEST_RE = re.compile(r"sha256:[0-9a-f]{64}")
NAME_RE = re.compile(r"rwml-oracle-[0-9a-f]{32}")


class ProcessFailed(ValueError):
    def __init__(self, status: int, stderr: bytes):
        super().__init__(f"oracle process failed with exit code {status}")
        self.status = status
        self.stderr = stderr


def canonical_json(value: object) -> bytes:
    return json.dumps(
        value, sort_keys=True, ensure_ascii=True, separators=(",", ":"), allow_nan=False
    ).encode()


def sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def load_runtime_lock(path: Path = DEFAULT_LOCK) -> dict:
    lock, _ = _load_json(path, 64 * 1024)
    _require_exact_keys(
        lock,
        {"schema", "version", "archive", "build", "files", "image"},
        "container lock",
    )
    if lock["schema"] != SCHEMA or lock["version"] != VERSION_LINE:
        raise ValueError("container lock version differs")
    archive = lock["archive"]
    if archive != {
        "url": ARCHIVE_URL,
        "bytes": 216816909,
        "sha256": "18838cb9d028b664a9d0e966cd4c8ca47ca3ea363c393b41d1b5124740b121a5",
    }:
        raise ValueError("container archive identity differs")
    if lock["build"] != {
        "platform": "linux/amd64",
        "source_date_epoch": 1783900800,
        "buildx_version": "0.35.0",
        "buildkit_image": "docker.io/moby/buildkit:v0.31.2@sha256:2f5adac4ecd194d9f8c10b7b5d7bceb5186853db1b26e5abd3a657af0b7e26ec",
        "base_image": "docker.io/library/debian:bookworm-slim@sha256:63a496b5d3b99214b39f5ed70eb71a61e590a77979c79cbee4faf991f8c0783e",
    }:
        raise ValueError("container build identity differs")
    files = lock["files"]
    if not isinstance(files, dict) or set(files) != set(RECIPE_FILES):
        raise ValueError("container recipe file set differs")
    for name, source in RECIPE_FILES.items():
        _require_sha256(files[name], "recipe digest")
        if sha256(read_regular_file(source, 64 * 1024)) != files[name]:
            raise ValueError(f"container recipe identity differs: {name}")
    image = lock["image"]
    if not isinstance(image, dict):
        raise ValueError("container image identity is invalid")
    _require_exact_keys(
        image, {"manifest_sha256", "config_sha256", "rootfs_sha256"}, "container image"
    )
    layers = image["rootfs_sha256"]
    if not isinstance(layers, list) or not 1 <= len(layers) <= 16:
        raise ValueError("container layer identity is invalid")
    for digest in [image["manifest_sha256"], image["config_sha256"], *layers]:
        _require_sha256(digest, "image digest")
        if digest == "0" * 64:
            raise ValueError("unbuilt image digest is invalid")
    return lock


def validate_image(info: dict, lock: dict) -> str:
    """Verify classic config-ID and containerd manifest-ID image stores."""
    expected = lock["image"]
    identifiers = {
        "sha256:" + expected[key] for key in ("manifest_sha256", "config_sha256")
    }
    if (
        not isinstance(info, dict)
        or not isinstance(info.get("Id"), str)
        or info["Id"] not in identifiers
    ):
        raise ValueError("loaded image digest differs")
    if info.get("Architecture") != "amd64" or info.get("Os") != "linux":
        raise ValueError("loaded image platform differs")
    descriptor = info.get("Descriptor")
    if descriptor is not None and (
        not isinstance(descriptor, dict)
        or descriptor.get("digest") != "sha256:" + expected["manifest_sha256"]
    ):
        raise ValueError("loaded image manifest differs")
    if info.get("Id") == "sha256:" + expected["manifest_sha256"] and descriptor is None:
        raise ValueError("manifest-addressed image has no descriptor")
    if info.get("RootFS") != {
        "Type": "layers",
        "Layers": ["sha256:" + digest for digest in expected["rootfs_sha256"]],
    }:
        raise ValueError("loaded image layers differ")
    config = info.get("Config")
    if not isinstance(config, dict) or any(
        config.get(key) != value
        for key, value in {
            "User": "65534:65534",
            "Entrypoint": ["/opt/rwml-oracle/capture.sh"],
            "WorkingDir": "/oracle",
        }.items()
    ):
        raise ValueError("loaded image execution configuration differs")
    return info["Id"]


def run_bounded(
    command: list[str],
    *,
    timeout: float = 30,
    stdout_limit: int = 1024 * 1024,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
) -> bytes:
    """Drain both pipes within byte/time limits; terminate the owned process group."""
    if os.name != "posix":
        raise ValueError("container capture currently requires a POSIX Docker client")
    if timeout <= 0 or not 0 < stdout_limit <= MAX_CAPTURE_BYTES:
        raise ValueError("process limits are invalid")
    output = {"stdout": bytearray(), "stderr": bytearray()}
    deadline = time.monotonic() + timeout
    with subprocess.Popen(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
        cwd=cwd,
        env=env,
    ) as process:
        try:
            with selectors.DefaultSelector() as selector:
                for label in output:
                    selector.register(
                        getattr(process, label), selectors.EVENT_READ, label
                    )
                while selector.get_map():
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise ValueError("oracle process timed out")
                    for key, _ in selector.select(min(remaining, 0.2)):
                        chunk = os.read(key.fileobj.fileno(), 65536)
                        if not chunk:
                            selector.unregister(key.fileobj)
                            continue
                        output[key.data].extend(chunk)
                        limit = stdout_limit if key.data == "stdout" else 65536
                        if len(output[key.data]) > limit:
                            raise ValueError("oracle process output exceeded its bound")
                try:
                    status = process.wait(
                        timeout=max(0.001, deadline - time.monotonic())
                    )
                except subprocess.TimeoutExpired as error:
                    raise ValueError("oracle process timed out") from error
                if status != 0:
                    raise ProcessFailed(status, bytes(output["stderr"]))
        finally:
            # Descendants may keep pipes alive after the client exits.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            except OSError as error:
                process.kill()
                process.wait()
                raise ValueError("oracle process group cleanup failed") from error
            process.wait()
    return bytes(output["stdout"])


def inspect_image(lock: dict) -> str:
    for key in ("manifest_sha256", "config_sha256"):
        reference = "sha256:" + lock["image"][key]
        try:
            payload = run_bounded(["docker", "image", "inspect", reference])
        except ProcessFailed:
            continue
        try:
            values = json.loads(payload)
        except (ValueError, UnicodeError) as error:
            raise ValueError("Docker image inspection is not JSON") from error
        if not isinstance(values, list) or len(values) != 1:
            raise ValueError("Docker image inspection is ambiguous")
        return validate_image(values[0], lock)
    raise ValueError(
        "locked image is unavailable; prepare, build, and load its archive first"
    )


def _mount(path: Path, destination: str) -> str:
    if any(character in str(path) for character in (",", "\n", "\r", "\x00")):
        raise ValueError("Docker mount path contains option delimiters")
    resolved = str(path.resolve(strict=True))
    if any(character in resolved for character in (",", "\n", "\r", "\x00")):
        raise ValueError("Docker mount path contains option delimiters")
    return f"type=bind,src={resolved},dst={destination},readonly"


def create_command(image: str, name: str, source: Path, fonts: Path) -> list[str]:
    if DIGEST_RE.fullmatch(image) is None or NAME_RE.fullmatch(name) is None:
        raise ValueError("container image or name is not canonical")
    return [
        "docker",
        "create",
        "--name",
        name,
        "--platform",
        "linux/amd64",
        "--pull",
        "never",
        "--network",
        "none",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--user",
        "65534:65534",
        "--cpus",
        "2",
        "--memory",
        "2g",
        "--memory-swap",
        "2g",
        "--pids-limit",
        "128",
        "--ulimit",
        "nofile=256:256",
        "--ulimit",
        "fsize=67108864:67108864",
        "--log-driver",
        "none",
        "--tmpfs",
        "/tmp:rw,nosuid,nodev,noexec,size=268435456,mode=1777",
        "--tmpfs",
        "/oracle/output:rw,nosuid,nodev,noexec,size=67108864,mode=1777",
        "--mount",
        _mount(source, "/oracle/source"),
        "--mount",
        _mount(fonts, "/oracle/fonts"),
        image,
    ]


def read_capture_archive(payload: bytes) -> dict[str, bytes]:
    if not payload or len(payload) > MAX_CAPTURE_BYTES:
        raise ValueError("capture archive size is outside the bound")
    result = {}
    try:
        with tarfile.open(fileobj=io.BytesIO(payload), mode="r:") as archive:
            for member in archive:
                if (
                    member.name not in CAPTURE_MEMBERS
                    or member.name in result
                    or not member.isfile()
                    or member.sparse is not None
                    or member.pax_headers
                ):
                    raise ValueError("capture archive member is invalid")
                maximum = MAX_PDF_BYTES if member.name == "output.pdf" else 65536
                if not 0 <= member.size <= maximum:
                    raise ValueError("capture member exceeds its bound")
                stream = archive.extractfile(member)
                if stream is None:
                    raise ValueError("capture member is unreadable")
                result[member.name] = stream.read(maximum + 1)
                if len(result[member.name]) != member.size:
                    raise ValueError("capture member is truncated")
    except (tarfile.TarError, OSError) as error:
        raise ValueError("capture archive is malformed") from error
    if set(result) != CAPTURE_MEMBERS:
        raise ValueError("capture archive is incomplete")
    return result


def run_container(
    command: list[str], name: str, *, timeout: float, stdout_limit: int
) -> bytes:
    """Run one named, bounded container and remove it on every exit path."""
    if NAME_RE.fullmatch(name) is None or command[:2] != ["docker", "create"]:
        raise ValueError("container execution request is invalid")
    created = False
    try:
        identifier = run_bounded(command).strip()
        created = True
        if re.fullmatch(rb"[0-9a-f]{64}", identifier) is None:
            raise ValueError("created container identity is invalid")
        payload = run_bounded(
            ["docker", "start", "--attach", name],
            timeout=timeout,
            stdout_limit=stdout_limit,
        )
        state = json.loads(
            run_bounded(["docker", "inspect", "--format", "{{json .State}}", name])
        )
        if (
            not isinstance(state, dict)
            or state.get("Running") is not False
            or type(state.get("ExitCode")) is not int
            or state["ExitCode"] != 0
            or state.get("OOMKilled") is not False
        ):
            raise ValueError("container did not complete successfully")
        return payload
    finally:
        # Even a failed client can have created a daemon-side container.
        try:
            run_bounded(["docker", "rm", "--force", name])
        except ProcessFailed as error:
            absent = any(
                line.endswith(b"No such container: " + name.encode())
                for line in error.stderr.splitlines()
            )
            if created or not absent:
                raise ValueError("oracle container cleanup failed") from None
        except ValueError:
            raise ValueError("oracle container cleanup failed") from None


def capture_document(image: str, source: Path, fonts: Path) -> dict[str, bytes]:
    name = "rwml-oracle-" + uuid.uuid4().hex
    payload = run_container(
        create_command(image, name, source, fonts),
        name,
        timeout=180,
        stdout_limit=MAX_CAPTURE_BYTES,
    )
    return read_capture_archive(payload)


def prepare_build(archive: Path, output: Path) -> None:
    lock = load_runtime_lock()
    payload = read_regular_file(archive, lock["archive"]["bytes"])
    if (
        len(payload) != lock["archive"]["bytes"]
        or sha256(payload) != lock["archive"]["sha256"]
    ):
        raise ValueError("LibreOffice archive identity differs")
    output.mkdir(parents=True, exist_ok=False)
    (output / "libreoffice.tar.gz").write_bytes(payload)
    for name, path in RECIPE_FILES.items():
        contents = read_regular_file(path, 65536)
        if sha256(contents) != lock["files"][name]:
            raise ValueError("recipe changed while preparing the build")
        (output / name).write_bytes(contents)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("--archive", type=Path, required=True)
    prepare.add_argument("--output", type=Path, required=True)
    commands.add_parser("inspect")
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            prepare_build(args.archive, args.output)
        else:
            print(inspect_image(load_runtime_lock()))
        return 0
    except (OSError, ValueError) as error:
        print(f"libreoffice_container: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
